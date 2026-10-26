#!/usr/bin/env python3
"""Ed25519 signing for the git update channel.

    scripts/release-key.py keygen KEY_FILE      # create the release key
    scripts/release-key.py pubkey KEY_FILE      # hex key to bake into the build
    scripts/release-key.py sign KEY_FILE FILE   # base64 signature on stdout
    scripts/release-key.py verify PUBKEY FILE SIG_FILE

The signed bytes are the file's exact bytes, with no re-serialisation, so what the
app verifies is byte-identical to what was signed and the JSON layout stays free to
change. Python is used because `cryptography` is already available at build time and
its ed25519 is a one-call primitive; the app verifies with ed25519-dalek.
"""
import base64
import pathlib
import sys

try:
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric.ed25519 import (
        Ed25519PrivateKey,
        Ed25519PublicKey,
    )
except ImportError:
    sys.exit("needs cryptography: pip install cryptography")

USAGE = __doc__


def public_hex(path):
    """Hex of the public half of a key file, whichever half was handed over.

    A raw ed25519 private key is 32 bytes and so is its public half, so the two
    cannot be told apart by content. Baking the private bytes as if they were a
    public key produces a key no signature ever verifies against, and the build
    accepts it without complaint, so the mistake only surfaces on a user's
    machine as "this build cannot verify the manifest". Anything not named
    ``*.pub`` is therefore read as the private key it came from and expanded.
    """
    if path.suffix == ".pub":
        key = Ed25519PublicKey.from_public_bytes(path.read_bytes())
    else:
        key = Ed25519PrivateKey.from_private_bytes(path.read_bytes()).public_key()
    return key.public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    ).hex()


def main(argv):
    if len(argv) < 2:
        sys.exit(USAGE)
    command, key_file = argv[0], pathlib.Path(argv[1])

    if command == "keygen":
        if key_file.exists():
            sys.exit(f"{key_file} exists; refusing to overwrite a release key")
        key = Ed25519PrivateKey.generate()
        key_file.parent.mkdir(parents=True, exist_ok=True)
        key_file.write_bytes(
            key.private_bytes(
                serialization.Encoding.Raw,
                serialization.PrivateFormat.Raw,
                serialization.NoEncryption(),
            )
        )
        key_file.chmod(0o600)
        public = key_file.with_suffix(".pub")
        public.write_bytes(
            key.public_key().public_bytes(
                serialization.Encoding.Raw, serialization.PublicFormat.Raw
            )
        )
        print(f"key:    {key_file}  (keep this private, never commit it)")
        print(f"pubkey: {public}")
        print(f"build with SHIFT_UPDATE_PUBKEY={public_hex(public)}")
        return 0

    if command == "pubkey":
        print(public_hex(key_file))
        return 0

    if command == "sign":
        if len(argv) < 3:
            sys.exit(USAGE)
        key = Ed25519PrivateKey.from_private_bytes(key_file.read_bytes())
        signature = key.sign(pathlib.Path(argv[2]).read_bytes())
        sys.stdout.write(base64.b64encode(signature).decode() + "\n")
        return 0

    if command == "verify":
        if len(argv) < 4:
            sys.exit(USAGE)
        signature = base64.b64decode(pathlib.Path(argv[3]).read_text().strip())
        Ed25519PublicKey.from_public_bytes(key_file.read_bytes()).verify(
            signature, pathlib.Path(argv[2]).read_bytes()
        )
        print("signature ok")
        return 0

    sys.exit(f"unknown command: {command}\n\n{USAGE}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
