# Shift 工程回归台账

本文件记录 0.1.2 到 0.1.10 之间已经踩过、并且不应再次引入的工程问题，重点覆盖：

- Drop / 局域网设备发现
- DOCX、Markdown 与图片转换
- 设置面板与「运行组件」的显示口径
- macOS DMG 打包及在其它机器上的安装

它记录的是实现约束、故障原因和回归方法，不是用户手册。修改相关代码或打包配置前，
先读对应章节；发布后如果得到新的有效结论，也把结论补回这里，并附上提交号。

## 版本线索

| 版本 | 本台账相关的已确认事实 |
| --- | --- |
| 0.1.2 | 当时的 Drop 在两台机器上实测可用；软件内安装升级和重启链路也已打通。 |
| 0.1.3 | 最近文件列表迁到应用数据目录，升级后不再随安装包替换而消失。 |
| 0.1.4 | 修复带图 DOCX→PDF、带相对图片 Markdown→DOCX/RTF 的丢图问题。 |
| 0.1.5 | macOS 改为 ad-hoc 签名并只发布通用 DMG，同时增加本地网络用途声明；但升级后出现 Drop 设备列表为空/只短暂闪现的回归。 |
| 0.1.6 | 增加按网卡定向广播，改善过滤组播的网络，但用户实测仍只能短暂看到设备。 |
| 0.1.7 | 增加单播扫描、普通信标回应、15 秒过期和 Windows 错误处理；用户实测 Mac 与 Windows 终端的 Drop 发现恢复正常。 |
| 0.1.8 | 最近文件改为收藏并移除拖回主操作按钮的交互；Windows 增加单实例保护；macOS DOCX→PDF 改用 Quick Look 源，保留页眉、页脚与装订版式。 |
| 0.1.9 | 重新支持把「最近使用」拖回三个主操作按钮，并且**按钮是唯一落点**——松开在面板其他位置不执行任何操作（见「『最近使用』拖回主面板的三个按钮」）；设置面板字号统一到 `--text-*` 阶梯；取消「截图后」行为选项，固定为结果卡片；修复 macOS「运行组件」把 AppKit 文档渲染器报成 Pandoc 缺失且没有安装入口的回归。 |
| 0.1.10 | 拖放反馈收束到按钮：面板自身不再做拖拽动画，松开在非按钮处彻底静默（去掉 0.1.9 的提示浮层）；「文字识别」对非图片直接拒绝，不再退回格式环；托盘「识别图片文字…」改为「文字识别」。 |

历史提交可以这样定位：

```bash
git show --stat 35cca59   # 0.1.6：定向广播
git show --stat 708bb5b   # 0.1.7：Drop 完整修复
git show --stat 19602b7   # 带图 DOCX→PDF
git show --stat 9950607   # Markdown 相对图片
git show --stat 42d6424   # macOS 改回 ad-hoc 签名
git show --stat 58949be   # 撤回 pkg，只保留 DMG
```

## Drop / 局域网设备发现

### 0.1.7 的发现链路

实现位于 `src-tauri/src/share.rs`。当前链路不是单独依赖组播，而是四层回退：

1. 每个运行中的 Shift 启动一个常驻 UDP 发现线程，绑定共享端口
   `47888`。
2. 常规信标每 2 秒发往组播组 `239.255.77.88:47888`，同时发往每个活动
   IPv4 网卡的定向广播地址和受限广播地址。
3. 打开 Drop 设备面板时，`share_refresh` 只设置
   `ShareState.refresh_requested` 原子标记，让常驻线程立刻广播并扫描；它不会再创建
   第二个同端口 socket。
4. 单播扫描覆盖本机所在网段，每 30 秒一次；大网段被限制在包含本机的 `/24`
   内，最多约 253 个地址，避免全网段泛洪。
5. 收到普通信标后，新版本总是回一个带 `reply: true` 的单播信标。回复信标不会再次
   触发回复，避免两台新版本无限互答。
6. 已知设备每 2 秒还会收到一次定向单播，因此即使对端自己的组播和广播都被过滤，
   只要先通过扫描建立联系，设备就能持续可见。
7. 15 秒没有再次收到设备信标才从列表移除。旧版本自己的过期时间是 9 秒，2 秒刷新
   周期必须在旧版本清除本机后仍来得及再次回应。
8. 文件传输使用信标中携带的动态 TCP 端口和 `SHSF` 握手；UDP 源端口不是传输端口。

### 不能随意改掉的约束

| 约束 | 原因 |
| --- | --- |
| `share_refresh` 必须复用常驻发现线程 | Windows 不能可靠地二次绑定 `47888`；Unix 上第二个 socket 可能抢走回复。0.1.7 之前的“只闪一下”就是这条链路失效的表现。 |
| 保留 `reply` 标记和 `#[serde(default)]` | 新版本必须能被旧版本解析，也必须能解析旧版本的省略字段；没有该标记，双方会无限互答。 |
| 普通信标每次都回复 | 只在“首次发现”时回复会让过滤组播的网络里的设备闪一次后消失。 |
| `PEER_TIMEOUT` 保持大于旧版 9 秒过期时间加刷新周期 | 0.1.7 使用 15 秒。缩短会重新制造旧版与新版本交替上线时列表闪烁的问题。 |
| 同时保留组播、定向广播、受限广播和单播扫描 | 不同路由器、访客 Wi-Fi 和企业网络会分别过滤其中一种；只改回其中一条路径会导致特定网络回归。 |
| 发现线程忽略 Windows 的 `ConnectionReset` / `ConnectionAborted` | 扫描关闭端口会产生排队的 ICMP 失败，这不是发现线程应退出的错误。 |
| 每 30 秒重建 socket，但不要退出线程 | 电脑休眠、切换 Wi-Fi 或网络接口变化后，需要重新加入组播；线程退出则再也不会恢复发现。 |
| `src-tauri/Info.plist` 保留 `NSLocalNetworkUsageDescription` | macOS 没有用途说明时无法正确发起本地网络访问授权。 |
| 发现端口和协议字段保持兼容 | 频道中同时可能有旧版本；新增字段必须可缺省，不能直接改掉现有 beacon 语义。 |

### 权限与网络排查

设备列表为空时，先按以下顺序排查，不要直接改发现算法：

1. **macOS 本地网络权限**：系统设置 → 隐私与安全性 → 本地网络，确认当前安装的
   `Shift.app` 已打开。ad-hoc 构建每次签名哈希可能变化，因此升级后可能出现新的
   Shift 条目，旧条目仍保留。
2. **Windows 防火墙**：首次运行时应允许 Shift 访问“专用网络”。公共网络配置下，
   Windows 防火墙通常会阻止入站信标和文件接收。
3. **客户端隔离**：访客 Wi-Fi 或企业网络若开启 AP/client isolation，两台设备即使
   在同一子网也无法直接收发 UDP/TCP。这是网络策略限制，不是端口扫描能绕过的。
4. **VPN 和多网卡**：确认两台机器使用的是同一个实际可互通网段；VPN 可能把默认路由
   和接口顺序带偏。当前扫描只覆盖活动 IPv4 网卡。
5. **Windows 单实例限制**：Windows 上每台机器只有一个 Shift 进程能绑定发现端口；
   验证多机发现时不要在同一台 Windows 上同时运行多个安装副本。
6. **ad-hoc 签名变化**：macOS 的本地网络授权绑定签名身份。无法解释的“升级前可见、
   升级后不可见”应先检查权限条目，再检查代码。

正常发现时日志中会出现：

```text
Shift share: found <machine> on <ip>
```

### 回归测试

自动测试在 `src-tauri/src/share.rs` 和 `src-tauri/Cargo.toml`：

```bash
cd src-tauri
cargo test --lib share::
```

重点用例：

- `subnet_scan_skips_this_host_and_broadcast`
- `subnet_scan_bounds_a_large_network_to_this_twenty_four`
- `reply_beacons_are_distinguished_from_regular_ones`
- `peer_timeout_covers_a_legacy_peers_reply_cycle`
- `two_copies_on_one_machine_discover_each_other`（默认 ignored，会绑定共享发现端口）

需要排查真实 socket 和当前网络时运行：

```bash
cd src-tauri
cargo test share:: -- --ignored --nocapture
```

每次修改发现、签名或打包配置后，仍要完成真机矩阵：

| 场景 | 验收标准 |
| --- | --- |
| macOS ↔ macOS | 打开 Drop 后数秒内出现对端，等待 15 秒以上仍不消失。 |
| macOS ↔ Windows x64 | Windows 允许专用网络防火墙后，双方都能发现对方。 |
| 升级后 | 旧版本升级到新版本，不需要重启路由器，设备列表能恢复。 |
| 切换 Wi-Fi / 休眠唤醒 | 30 秒内重新发现；不得要求退出应用。 |
| 权限拒绝再允许 | macOS 本地网络或 Windows 防火墙从拒绝改为允许后，重新打开面板可发现。 |

0.1.7 的用户验收环境是 Mac 与 Windows 各一台，升级后 Drop 设备发现正常。该结论
覆盖真实跨系统发现，但仓库所在平台没有 CI，也没有自动化的双机网络夹具，因此发布前
仍需重复上面的手工矩阵。

## DOCX、Markdown 与图片转换

### 当前转换路径

| 输入 → 输出 | 当前实现 | 关键点 |
| --- | --- | --- |
| 纯图片 DOCX → PDF | `office::document_page_images` 按文档顺序抽取页面图片，再由 `pdf::images_to_pdf_with_progress` 生成 PDF | 保留页数和原始图片内容；这是扫描件路径，不经过 AppKit 文本排版。 |
| 普通 DOCX → PDF | 先用 `/usr/bin/qlmanage -p` 生成系统的 Quick Look `Preview.html`，再由 `native/macos/doc-render.swift` 用 WebKit 渲染并按 A4 切片 | AppKit 的 OOXML 读取器只保留正文；把 Quick Look HTML 再转成 attributed string 还会重排表格并制造空白页。`qlmanage` 有 30 秒超时，且必须确认 `Preview.html` 存在。 |
| Quick Look 不可用且含图 DOCX → PDF | 用 Pandoc `docx → html5`，通过 `--extract-media` 抽取图片到临时目录，再交给 `doc-render` | 保留图片的降级路径；Quick Look 失败或超时后才进入。 |
| Markdown → PDF | 先由 Pandoc（可用时）或内置 `markdown::to_html` 转成 HTML，再交给 WebKit 渲染器；HTML 的 base URL 指向 Markdown 源目录 | 相对图片路径依赖源目录，不能用临时 HTML 所在目录。 |
| Markdown → DOCX / RTF | Pandoc，并传 `--resource-path <源目录>` | Pandoc 的工作目录不是文档目录；缺少该参数会把相对图片替换成 alt 文本。 |
| Markdown → HTML | Pandoc，并传 `--resource-path <源目录>` 和 `--self-contained` | 输出必须是可单独移动的单文件，不能引用从未生成的 `media/` 目录。 |
| 含图 DOCX → HTML | Pandoc 加 `--self-contained` | 图片以内嵌 data URI 写入 HTML。 |
| 任意含宽图的文档 → PDF | HTML 在 WebKit 测量前把超宽 `<img>` 等比缩到页面宽度；非 HTML 文档仍由 `fitAttachments` 调整 attachment bounds | 不重采样像素，只修正排版请求的尺寸，避免截图被裁成碎片。 |

### 不能随意改掉的约束

1. **普通 DOCX 不能直接交给 AppKit OOXML 读取器。** 它只保留正文，页眉页脚会消失；
   含图文档还会返回只有文字、没有附件的 attributed string。必须先走 Quick Look，
   失败后才允许回退。
2. **HTML 必须交给 WebKit，不能再转成 NSAttributedString。** 后者会重新排版 Quick
   Look 的定位内容，把原本同页的页脚推到新页，并把 7 页内容拉成 9 页。
3. **纯图片 DOCX 的页面抽取要排在通用渲染之前。** 它决定扫描件的真实页数和页面顺序。
4. **DOCX→HTML 后必须把 `--extract-media` 的临时目录作为 HTML base directory。**
   Pandoc 会把图片写成相对路径；base 指向源 DOCX 目录会找不到已抽取媒体。
5. **Markdown→PDF 的 HTML base URL 始终是源 Markdown 目录。** 即使 HTML 写在临时目录，
   也不能改用临时目录。
6. **Pandoc 转 DOCX/RTF/HTML 必须保留 `--resource-path`。** 这是相对图片丢失问题的
   根因修复，不应为了简化命令删掉。
7. **HTML 输出必须保留 `--self-contained`。** 否则用户拿到的是一个引用不存在媒体目录
   的 HTML 文件。
8. **宽图适配在 `doc-render.swift` 中完成。** HTML 必须在测量前缩放宽图；非 HTML
   文档的 AppKit `<img>` 单元格不会服从 CSS width，仍需调整 attachment bounds。
9. **无 Pandoc 时保留 macOS 的降级路径。** 带图 DOCX 会退回旧的文本渲染，而不是让
   整个转换直接失败；修改时要明确产品选择，不能无意打破降级。
10. **Swift 渲染器改动后重新运行 `pnpm mac:drag-bridge`。** 只改源码不重编伴生进程，
   打出的 DMG 不会包含修复。
11. **Quick Look 临时目录必须在所有成功和失败路径清理。** 输出先写到临时目录，确认
    PDF 有效后再改名；不能把半成品留在用户的输出目录。

### 回归测试

自动测试集中在 `src-tauri/src/files.rs`：

- `renders_image_only_docx_pages_to_pdf`
- `keeps_pictures_when_rendering_a_text_docx_to_pdf`
- `fits_a_wide_picture_onto_the_page`
- `keeps_docx_headers_and_footers_when_converting_to_pdf`
- `image_bearing_docx_to_html_embeds_images`
- `keeps_markdown_images_when_converting_to_word`

## 升级后的单实例

Windows 安装器重启应用时可能留下旧进程，再启动新进程就会有两个托盘实例和两套窗口。
`tauri-plugin-single-instance` 必须在 `tauri::Builder` 中**最先注册**：第二个进程在
其余插件和窗口初始化前就把参数交给第一实例并退出，第一实例收到回调后显示主窗口。

macOS 的 `hotkeys::acquire_instance` 文件锁仍保留为热键所有权的额外保护，不能因为加入
单实例插件而删除；但它不是 Windows 的解决方案，非 Unix 分支本身不会持有跨进程锁。

其中 `keeps_markdown_images_when_converting_to_word` 覆盖 DOCX 和 RTF；
`image_bearing_docx_to_html_embeds_images` 检查 HTML 中的图片是 data URI 而不是
`media/` 引用。很多用例在缺少 Pandoc 时跳过，因此本机安装 Pandoc 后才能得到完整结果。

历史验收样例：

- `平台技术方案(带图测试专用).docx`
- `智飞空地一体化低空应用平台技术方案.md`

这两个样例是手工验收材料，当前没有提交到仓库，不能声称 CI 会自动重跑。每次改动转换
路径时，应重新用它们检查标题、表格、正文顺序、相对路径图片和宽图是否同时保留。

### 当前边界

- 自动化覆盖的是本地文件和相对路径图片。没有单独的 HTTP/HTTPS 图片下载、缓存、重试
  与离线回退实现，因此不要声称任意远程图片 URL 都已有自动回归保障。
- 含图 DOCX 依赖 Pandoc；没有 Pandoc 时会退回旧文本路径，这是兼容性降级，不等于
  图片仍被保留。
- AppKit 渲染器只在 macOS 构建中可用；Windows/Linux 的文档→PDF 仍走各自的 Pandoc
  与 PDF 引擎路径。

## macOS DMG 打包与跨机器安装

### 当前发布方案

Shift 的 macOS 产物只有：

```text
Shift-<版本>-macOS-universal.dmg
```

它是 Apple silicon 与 Intel 通用的应用包，同时用于首次安装和软件内升级。更新清单的
`macos` 键必须指向这个 DMG，因为软件内升级脚本会用 `hdiutil` 挂载它再取出应用。
不再发布 `.pkg`；旧的 `scripts/macos-pkg.sh` 已从仓库删除，任何 `.pkg` 都不能进入
正式发布目录或更新清单。

当前签名配置是：

```json
"macOS": {
  "signingIdentity": "-",
  "hardenedRuntime": false
}
```

`"-"` 表示 ad-hoc 签名。项目没有 Apple Developer ID，也不做 notarization，因此新机器
第一次打开仍需在 Gatekeeper 中授权一次。

### 为什么不能用本地自签名证书

历史上曾使用构建机钥匙串中的 `Shift Local Signing` 证书。它在构建机上看起来完全
正常：`codesign` 验证通过，`spctl` 也可能接受，因为证书只存在于那台机器。换到其它
Mac 后，证书无法链到受信任根，下载的应用会被系统报告为“已损坏”或恶意软件，而且只
提供“移到废纸篓”，没有允许运行的入口。

因此：

- **不要重新启用 `Shift Local Signing` 或任何只存在于构建机的证书。**
- **不要清空 `signingIdentity` 让它自动选择钥匙串身份。** Tauri 可能挑到本机证书，
  重新产生同样的跨机器故障。
- 保持 ad-hoc，直到项目真正具备 `Developer ID Application` 证书和 notarization。

### 首次安装与软件内升级

| 场景 | 预期行为 |
| --- | --- |
| 其它 Mac 首次下载 DMG | 双击挂载，拖 `Shift.app` 到 `/Applications`。首次启动会提示“身份不明的开发者”；右键 → 打开，或在系统设置 → 隐私与安全性中允许一次。 |
| 软件内升级 | 下载 DMG、校验字节数与 SHA256，退出应用后由 helper 挂载、复制、移除 quarantine、替换并重启；不经过浏览器，因此不应出现 Gatekeeper 提示。 |
| 构建机本地运行 | 能打开不代表发布包合格；必须从 DMG 转移到另一台机器验证，构建机结论不能替代跨机器验收。 |

### 每次 macOS 发布前的硬性检查

1. 运行 `pnpm mac:drag-bridge`，确保拖拽、PDF 与文档渲染三个 Swift 伴生进程都已重新
   编译为 universal。
2. 构建 `pnpm tauri build --target universal-apple-darwin --bundles app,dmg`。
3. 对应用执行 `codesign --verify --deep --strict`，并确认
   `codesign -dv --verbose=2` 显示 `Signature=adhoc`，而不是本地证书名称。
4. 对 DMG 执行 `hdiutil verify`；对发布目录执行 `shasum -a 256 -c SHA256SUMS.txt`。
5. 确认 `artifacts/` 和 `shift-update.json` 都没有 `.pkg`，`macos` 键指向 DMG。
6. **把 DMG 转移到另一台 Mac 验证。** 最好使用没有安装过 Shift 的机器，并通过浏览器从
   发布源下载最终文件，让 quarantine 标记符合真实用户路径。若用 USB 或 AirDrop 转移，
   应确认隔离属性仍存在，否则 Gatekeeper 这一项没有被验证。完成拖入 `/Applications`、
   首次右键打开、启动应用，并检查版本号。
7. 在同一台机器打开 Drop，确认本地网络授权正常、能看到另一台 Shift 终端。签名和打包
   改动最容易同时影响应用启动与局域网权限。
8. 如果验证的是升级包，再从旧版本执行一次软件内升级并确认无需 Gatekeeper 提示。

第 6 步是发布闸门。只在一台机器上生成、签名和打开 DMG 不足以证明“其它机器可安装”。
0.1.7 的 DMG 已在其它机器验证可用；后续每次更换签名方式或打包容器时都要重做。

### 已知边界

- ad-hoc 构建每次的签名哈希可能变化，macOS 的“屏幕录制”和“本地网络”授权也可能重置。
  这是当前没有 Developer ID 的代价，不是 Drop 代码回归。
- Developer ID + notarization 才能消除首次安装提示，并让权限在升级间稳定。当前项目没有
  该证书，不能把“已公证”写入发布说明。
- 发布前不能只看构建机上的 `spctl` 结果；跨机器安装行为和系统提示类型才是有效证据。

## 截图、录制与圆环的键盘焦点

### 症状与真实原因

区域截图、区域录制时按 `ESC` 无法取消，拖动文件后出现的圆环按 `ESC` 也无反应。前端确实
监听了 `keydown`，但事件根本没有到达页面。

在运行中的 Shift 上用 `lldb` 取证可以看到：

```text
keyWindow      = <TaoWindow "Shift Capture">   isKeyWindow = YES
firstResponder = <TaoView>          ← 页面在 contentView 的 subview 里，不在这里
contentView    = <TaoView>  subviews = [<WryWebView>]
```

窗口是 key window，但 AppKit 的键盘事件投递给的是窗口的 first responder，而 first
responder 停在 tao 自己的 `TaoView` 上，所以页面收不到任何按键。链路上有两个成因：

- `WebviewWindow::set_focus()` 只做 `makeKeyAndOrderFront` 加激活应用
  （tao `platform_impl/macos/window.rs` 的 `set_focus`），不会动 first responder。
- wry 在创建 webview 时调用过一次 `makeFirstResponder(webview)`，但之后每次
  `set_simple_fullscreen` 都会经 `toggle_style_mask` 再调
  `makeFirstResponder(ns_view)`（tao `util/mod.rs`）把它抢回去；窗口重新显示同样会丢。

因此**只要窗口被显示过一次，就必须再显式把键盘交给页面**。等价验证：在真实进程里执行
`makeFirstResponder(WryWebView)` 之后，同一个 `ESC` 立刻被页面收到。

### 不能随意改掉的约束

| 约束 | 原因 |
| --- | --- |
| 显示 overlay / 圆环后必须调用 `crate::focus_page` | 只有 `Webview::set_focus`（`makeFirstResponder`）能让页面收到 `ESC`/`Enter`；`WebviewWindow::set_focus` 不够。 |
| 采集前记住哪些 Shift 窗口是可见的，结束时按名单恢复 | 之前无条件 `show` 主窗口：面板开着会被截图吃掉，面板本来关着又会被凭空打开。 |
| 录制期间不能恢复面板，`recording::stop` 才恢复 | 恢复出来的窗口会出现在被录制的区域里。 |
| `restore_windows` 用 recorder HUD 判断是否在录制，不能去锁 `state.recording` | `recording::stop` 全程持有该 Mutex，从这条路再锁一次会自死锁。 |
| 已有 overlay 或正在录制时，新一轮采集不得覆盖待恢复名单 | 否则后一次（面板不可见）会把“面板开着”的记录擦掉。 |

### 回归测试

`src-tauri/src/lib.rs` 中的源码断言守住调用点：

```bash
cd src-tauri
cargo test --lib capture
```

键盘投递本身只能实测，最小矩阵（每步都要看窗口是否真的消失/回来）：

1. 面板开着 → `Alt+Shift+2` → 直接按 `ESC` → overlay 消失且面板回来。
2. 面板开着 → `Alt+Shift+3` → 直接按 `ESC` → 同上。
3. 面板关着 → `Alt+Shift+2` → `ESC` → 不要凭空出现面板。
4. 面板开着 → 选区录制 → 录制中面板保持隐藏 → 停止后面板回来，`.mov` 有效。
5. 录制中再截一张图 → `ESC` 只关 overlay，录制与 HUD 不受影响。
6. 圆环（拖文件到面板，或「文件转换」选文件后）→ `ESC` 收起圆环。
7. `Enter` 仍能确认选区并保存截图。

## 「运行组件」的显示口径

### 0.1.9 的 macOS 回归

macOS 的设置 → 运行组件里，文档转 PDF 那一行显示 **“Pandoc PDF pipeline / 缺失”**，
而且**没有安装按钮**。两件事都是同一个错误造成的。

`document_pdf_component()` 原本在 `if let Some(renderer) = files::doc_render_bridge()`
里 `return` 掉内置的 AppKit 渲染器；`df35111`（准备 0.1.8）把 `return` 改成了裸表达式，
于是构造出的组件被丢弃、继续往下走到 Pandoc 分支。后果是：

- 名字变成 `Pandoc PDF pipeline`，可用条件变成 `pandoc.is_some() && pdf_engine.is_some()`；
  macOS 这条链路根本不需要 LaTeX/Typst 引擎（pandoc 只负责把带图 DOCX 转成 HTML 交给
  渲染器），所以只要机器上没装 xelatex/typst 之类就永远显示缺失。
- 面板对 `external` 且缺失的行只给安装按钮，而 `install_target("document-pdf")` 在
  macOS 没有条目（只有 `pandoc`、`media`），于是既报缺失又无处可装。

**修法不是补一个安装入口，而是恢复「macOS 由内置渲染器作答」这件事。** 现在两个平台各自
拥有独立的 `document_pdf_component()`（`#[cfg(target_os = "macos")]` /
`#[cfg(not(target_os = "macos"))]`），与同文件里 `pdf_page_component`、
`image_backend_component` 的写法一致：在 macOS 上想再次落到 Pandoc 分支会变成编译错误，
而不再是一次静默的行为回归。

### 不能随意改掉的约束

| 约束 | 原因 |
| --- | --- |
| macOS 的 `document-pdf` 必须是 `source = "bundled"` 且以 `doc_render_bridge()` 为准 | 这条转换在 macOS 上就是 Quick Look / AppKit 渲染器；改成 external 会重新引入"缺失但装不了"。 |
| 带图 DOCX 仍需要 Pandoc，但那是 `pandoc` 那一行的职责 | `pandoc` 行有自己的安装目标（`brew install pandoc`）；不要把依赖合并进 `document-pdf`。 |
| `external` + 缺失的行必须有安装目标 | 否则面板等于告诉用户坏了、却不给出口，就是本次回归的表现。 |

### 回归测试

`src-tauri/src/runtime.rs` 的 `tests` 模块：

```bash
cd src-tauri
cargo test --lib runtime
```

- `macos_document_pdf_is_the_bundled_renderer_not_a_pandoc_pipeline`：钉住 macOS 的
  `source` 与 `name`。
- `a_missing_component_always_offers_a_way_forward`：任何标记为缺失的行都必须要么可安装、
  要么本来就不是 external。

## 「最近使用」拖回主面板的三个按钮

0.1.8 曾经把这条交互关掉：一行拖出去是文件，拖回来却什么都不是，用户只能重新选一次文件。
现在拖到「文件转换 / 文件工具 / 文字识别」上就执行该按钮的动作。

**按钮是唯一的落点，也是唯一会响应拖放的东西。** 这条规则分两步收紧：

- 0.1.9 去掉了"落在面板空白处就打开操作环"的兜底，改成不执行操作、弹一条提示。
- 0.1.10 连提示也去掉：既然不执行任何操作，解释"为什么没执行"的浮层只是在用户没有松手的位置
  制造动静，落在面板其他位置现在是彻底静默。

外部拖入和内部拖拽始终遵循同一条规则，`dropFiles` 因此接受 `DropAction | null`，`null` 直接
原样返回——它不是"未知动作"，而是"没有动作"，两条投递路径共用这一个判断。

同理，拖拽反馈只加在按钮上：进行中三个按钮同时点亮为候选落点（原生拖拽用 `dragging`，内部
拖拽用 `rowDragging`），而 `.home-drop` 面板本身不再缩放、变色或换描边——`is-dragging` 类、
对应的两条 CSS 规则、以及随之失效的 `transition` 都已删除，标题也不再改文案。从外部拖入和从
列表拖起现在是同一种观感，之前只有外部拖入会让整块面板动起来。实现上有一条约束容易在重构时
丢掉：

**一次内部拖拽可能被报告两次。** 历史行使用 `dragOutPayload`，也就是 `text/uri-list` 里的
`file://` 加纯文本路径，因此它对外就是一个文件拖拽。同一次手势有两种投递路径：

- DOM：`onDragOver` / `onDrop`（挂在 `.home` 根节点，用 `clientX/clientY` 命中按钮）；
- 原生：`getCurrentWebview().onDragDropEvent` 的 `drop`（坐标要先经 `toWindowPoint` 换算）。

哪个先、是否都会到，取决于 WebView 版本与平台，所以两条路都必须武装，再由
`recentDrag.current = { path, landed }` 的 `landed` 标记只放行第一个。去掉这个标记，
圆环会被打开两次；只保留 DOM，Windows 的原生投递会让拖拽失效。

另外两条约束：

| 约束 | 原因 |
| --- | --- |
| `onDragEnd` 不能立即清空 `recentDrag`，要延迟 500 ms | 部分引擎在按钮抬起之后才发出原生 `drop`，提前清空会让这一次手势变成"落回面板的空拖拽"。 |
| 两条投递路径都必须把"没命中按钮"当作同一种结果处理 | 内部拖拽与外部拖入的语义必须一致；只在 DOM 路径静默、原生路径仍兜底开环或弹提示，就会出现同一个动作两种结果。 |
| 点亮候选按钮的开关有两个来源 | 外部拖拽靠原生 `enter` 设置的 `dragging`，内部拖拽原生事件不一定到，必须另有 `rowDragging`，否则从列表拖起时按钮毫无反应。 |
| 面板不要用 `is-dragging` 自己做拖放反馈 | 内部拖拽拿不到原生事件，只有外部拖拽会点亮面板，两种拖法观感不同；把反馈收在按钮上，两条路径共用同一套 `has-drop-target`。 |
| 「文字识别」不接受兜底 | `runOcrOn` 遇到非图片直接拒绝并提示，不再转交格式环：按钮说做什么就做什么，"拖到 OCR 却弹出转换环"会让用户以为按钮被忽略了。 |

### 回归测试

`src/lib/dropTargets.test.ts` 覆盖命中区域与坐标换算；拖拽本身用浏览器实测（Chromium 与
WKWebView 两条路径都要走一遍）：

1. 「最近使用」某行 → 拖到「文件转换」→ 只弹出一个格式环，且文件是这一行。
2. 同一行 → 拖到「文件工具」→ 只弹出工具环。
3. 图片行 → 拖到「文字识别」→ 打开编辑器并自动开始识别；非图片行 → 不打开编辑器、不开环，只提示"文字识别仅支持图片格式"。
4. 同一行 → 拖到面板空白处 / 标题栏 / 列表本身 → 不开环、不执行、**不出现任何提示浮层**。
5. 从访达拖文件到面板空白处 → 同样不执行、不提示；拖到任一按钮 → 执行该按钮的动作。
6. 行拖出到访达/聊天窗口 → 仍是文件本体，没有被 Shift 自己吃掉。
7. 两种拖法各拖一次 → 面板（`.home-drop`）在拖拽全程不缩放、不变色，标题文案不变；只有三个按钮在拖拽开始时同时亮起虚线候选态、松开后熄灭。

## 修改这些路径时的统一回归命令

```bash
git diff --check

cd src-tauri
cargo fmt --check
cargo test --lib

cd ..
pnpm check
pnpm test
```

发布构建还需额外完成本文对应章节的手工矩阵。任何一次修改如果改变了发现协议、转换
依赖链或 macOS 签名方式，都应在本文件补一条带日期的结论和提交号。
