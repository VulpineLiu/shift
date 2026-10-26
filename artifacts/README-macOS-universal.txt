Shift 0.1.3 - macOS 通用（universal）

本文件说明 macOS 安装包是怎么打出来的、两个安装包各自负责什么、在其它机器上
会遇到什么，以及如何自己校验。完整细节见仓库 docs/RELEASE.md 的「macOS packaging」。


为什么有两个安装包
------------------

本项目的构建是 ad-hoc 签名（src-tauri/tauri.conf.json 里 signingIdentity 固定为
"-"），且没有 Apple Developer ID 可做公证。Gatekeeper 因此无法为任何 Shift 构建背书，
一台新机器上的表现取决于安装包的形式：

| 安装包 | 用途 | 新机器上的表现 |
| --- | --- | --- |
| Shift-macOS-universal.pkg | 首次安装，交给任何新机器 | installer 不会把隔离属性带给装好的应用，装好后打开**完全没有提示**。已实测：装好的副本只带 com.apple.provenance，没有 com.apple.quarantine。 |
| Shift-macOS-universal.dmg | 软件内升级通道（清单里的 macos 键） | 应用自己下载的副本不带隔离属性，所以软件内升级也不弹窗。若是从浏览器下载再拖进 /Applications，首次打开需要允许一次。 |

所以：新机器装 .pkg，已在用的 Shift 走软件内升级（下载的是 .dmg）。清单的 macos
键必须是 .dmg —— 软件内升级的交接脚本用 hdiutil 挂载镜像再取出应用包，指向 .pkg
会让每次升级都挂载失败。

pkg 里的应用与被拖出来的应用是同一份签名：pkgbuild 既不重新签名也不剥离签名，
内置应用、DMG 内副本、pkg 内副本的 CDHash 完全一致（已核对）。


在其它机器上安装
----------------

安装包 .pkg（推荐，新机器）
  1. 下载 Shift-macOS-universal.pkg。pkg 本身没有签名（productsign 需要一枚带
     Installer Signing 用途的证书，本项目只有代码签名身份），因此浏览器下载后
     双击可能被拦：右键 → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」，
     只需一次，发生在安装之前。
  2. 之后按提示安装，载荷会落到 /Applications。
  3. 启动 Shift，不会再有任何 Gatekeeper 提示。
  终端安装连这一次都不需要：
    sudo installer -pkg Shift-macOS-universal.pkg -target /

磁盘镜像 .dmg（新机器，或想手动装）
  1. 双击挂载，把 Shift.app 拖进 /Applications。
  2. 首次启动被拦：「无法打开，因为来自身份不明的开发者」。点取消后，右键
     Shift.app → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」。
  3. 只需允许一次，之后再打开正常。

macOS 权限
  截图与录屏需要「屏幕录制」，输出目录需要「文件与文件夹」，带声音录屏需要
  「麦克风」。Shift 会在打开截图遮罩前先检查屏幕录制权限，被拒绝时给的是可操作
  的提示，而不是一张全黑截图。
  注意：ad-hoc 构建每次的代码签名哈希都不同，而 macOS 把「屏幕录制」这类授权绑在
  这个哈希上，所以一次升级后可能需要重新授权——这与手动重装是同一后果。要彻底消除
  它，需要 Developer ID 证书加公证（见仓库 docs/ROADMAP.md 的 M5）。

软件内升级
  设置 → 软件更新 里检查并安装：Shift 自己下载 .dmg，先核对字节数与 SHA256，
  再由一个独立脚本等本进程退出后挂载镜像、ditto 出新副本、去掉隔离属性、用两次
  mv 换上新版本并重新打开。下载不走浏览器，新副本不带隔离属性，所以这一步没有任何
  Gatekeeper 提示。


如何自己构建
------------

构建机需先装好两个 Rust 目标，以及 macOS 侧的原生伴生进程（AppKit 拖拽桥、PDFKit
与文档渲染，会被打成通用二进制）：

    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    pnpm install
    pnpm mac:drag-bridge

生成通用应用与 DMG，再把应用包包成 .pkg：

    pnpm tauri build --target universal-apple-darwin --bundles app,dmg
    scripts/macos-pkg.sh \
      src-tauri/target/universal-apple-darwin/release/bundle/macos/Shift.app \
      artifacts/Shift-macOS-universal.pkg

scripts/macos-pkg.sh 把应用包暂存到一个 /Applications 根目录下，再用 pkgbuild 打包；
install-location 指向 /Applications，所以这台机器上的每个用户都会装到那里。它用
ditto 而不是 cp，以保留资源分支、符号链接与代码签名。

若希望更新通道能校验来源，构建时把 ed25519 公钥编进去（发布流程见仓库
docs/RELEASE.md 的「Signing the channel」）：

    export SHIFT_RELEASE_KEY=~/.config/shift-release/release.ed25519
    export SHIFT_UPDATE_PUBKEY=$(python3 scripts/release-key.py pubkey \
        ~/.config/shift-release/release.ed25519)


如何校验
--------

哈希（本目录下的 SHA256SUMS.txt 覆盖所有产物）：

    shasum -a 256 -c SHA256SUMS.txt

签名形态（adhoc 是当前刻意选择的形式，不是出错）：

    codesign -dv --verbose=2 /Applications/Shift.app 2>&1 | grep -E 'Signature|Identifier'
    #   Signature=adhoc
    #   Identifier=app.nex.shift

安装包本体（pkg 未签名是已知状态，真正的签名在里面的应用上）：

    pkgutil --check-signature Shift-macOS-universal.pkg   # Status: no signature
    pkgutil --payload-files Shift-macOS-universal.pkg     # ./Shift.app/...

镜像完整性：

    hdiutil verify Shift-macOS-universal.dmg

校验后仍打不开的话，请确认不是在压缩包预览窗口里运行的，并优先使用 .pkg；若坚持用
.dmg，按上面的「在其它机器上安装」处理首次允许。
