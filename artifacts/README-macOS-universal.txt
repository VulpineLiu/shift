Shift 0.1.12 - macOS 通用（universal）

本文件说明 macOS 安装包是怎么打出来的、在其它机器上会遇到什么，以及如何自己校验。
完整细节见仓库 docs/RELEASE.md 的「macOS packaging」和 docs/ENGINEERING-NOTES.md
的「macOS DMG 打包与跨机器安装」。


关于这个安装包
--------------

Shift 的 macOS 产物只有一个：Shift-0.1.12-macOS-universal.dmg，通用二进制，Apple silicon 与
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
  1. 下载并双击 Shift-0.1.12-macOS-universal.dmg 挂载镜像。
  2. 把 Shift.app 拖进「应用程序」（或任何你有权限的位置）。
  3. 首次启动会被拦：「无法打开，因为来自身份不明的开发者」。点取消，然后
     右键 Shift.app → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」。
  4. 只需允许一次，之后再打开正常。

macOS 权限
  截图与录屏需要「屏幕录制」，输出目录需要「文件与文件夹」，带声音录屏需要
  「麦克风」，「发送到设备」需要「本地网络」。Shift 会在打开截图遮罩前先检查屏幕
  录制权限，被拒绝时给的是可操作的提示，而不是一张全黑截图。
  「本地网络」被拒绝时 Shift 无法可靠地发送或接收设备信标，设备列表会一直是空的。
  若看不到其它终端，请到「系统设置 → 隐私与安全性 → 本地网络」确认当前安装的
  Shift 是打开的；那里可能留着多条目，因为每次重新构建都是一个新身份，请对准当前
  安装的那一个。
  注意：ad-hoc 构建每次的代码签名哈希都不同，而 macOS 把「屏幕录制」「本地网络」
  这类授权绑在这个哈希上，所以一次升级后可能需要重新授权——这与手动重装是同一
  后果。要彻底消除它，需要 Developer ID 证书加公证（见仓库 docs/ROADMAP.md 的 M5）。

发布前的硬性回归
  构建机能够打开自己生成的 DMG，不代表发布包合格。每次发版前必须通过发布源在另
  一台 Mac（最好从未安装过 Shift）用浏览器下载最终 DMG，以保留真实用户会遇到的
  quarantine 标记；然后挂载、拖入 Applications、完成首次授权并启动，确认版本号
  正确。随后打开 Drop，确认能看到另一台 Shift 终端且不会很快消失。若该版本同时
  提供软件内升级，还要从上一版实际升级一次，确认不会出现 Gatekeeper 提示。任何
  一项未通过都不得上传 DMG。

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

`pnpm mac:release` 会构建、校验并复制为 artifacts/Shift-0.1.12-macOS-universal.dmg。
发布流程（清单、签名、推送）见仓库 docs/RELEASE.md。

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

    hdiutil verify Shift-0.1.12-macOS-universal.dmg

校验后仍打不开的话，请确认不是在压缩包预览窗口里运行的，并按上面的「新机器首次安装」
处理首次允许：右键 → 打开，或在「系统设置 → 隐私与安全性」中点「仍要打开」。
