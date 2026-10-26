Shift 0.1.3 - macOS 通用（universal）

本文件说明 macOS 安装包是怎么打出来的、在其它机器上会遇到什么，以及如何自己校验。
完整细节见仓库 docs/RELEASE.md 的「macOS packaging」。


关于这个安装包
--------------

Shift 的 macOS 产物只有一个：Shift-macOS-universal.dmg，通用二进制，Apple silicon 与
Intel 都能用。它既是新机器的首次安装方式，也是软件内升级的载体（更新清单里的 macos
键指向它）。清单的键必须是 .dmg，因为软件内升级的交接脚本要用 hdiutil 挂载镜像再
取出应用包；早期曾随包发布的 .pkg 在验证过 DMG 可用后就撤掉了，不为它多维护一份
产物。

本项目的构建是 ad-hoc 签名（src-tauri/tauri.conf.json 里 signingIdentity 固定为
"-"），且没有 Apple Developer ID 可做公证。Gatekeeper 因此无法为任何 Shift 构建背书，
一台新机器上会看到「身份不明的开发者」提示——这是可以放行的，见下节。


在其它机器上安装
----------------

新机器首次安装
  1. 下载并双击 Shift-macOS-universal.dmg 挂载镜像。
  2. 把 Shift.app 拖进「应用程序」（或任何你有权限的位置）。
  3. 首次启动会被拦：「无法打开，因为来自身份不明的开发者」。点取消，然后
     右键 Shift.app → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」。
  4. 只需允许一次，之后再打开正常。

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

生成通用应用与 DMG：

    pnpm tauri build --target universal-apple-darwin --bundles app,dmg

产物在 src-tauri/target/universal-apple-darwin/release/bundle/macos/ 下：Shift.app
与 Shift-macOS-universal.dmg。发布流程（清单、签名、推送）见仓库 docs/RELEASE.md。

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

镜像完整性：

    hdiutil verify Shift-macOS-universal.dmg

校验后仍打不开的话，请确认不是在压缩包预览窗口里运行的，并按上面的「新机器首次安装」
处理首次允许：右键 → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」。
