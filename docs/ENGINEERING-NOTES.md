# Shift 工程回归台账

本文件记录 0.1.2 到 0.1.13 之间已经踩过、并且不应再次引入的工程问题，重点覆盖：

- Drop / 局域网设备发现
- DOCX、Markdown 与图片转换
- 设置面板与「运行组件」的显示口径，以及 Windows 上的一键安装组件
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
| 0.1.11 | Windows 实测「最近使用」拖到三个按钮全部完不成：页内文件拖拽在 WebView2 上 DOM 与原生两条投递路径都可能不上报，改由 `dragEnd` 兜底；开始拖动时先让闲置圆环让开，它无边框且始终置顶，正压在按钮上（两节都在「『最近使用』拖回主面板的三个按钮」里）。三个按钮的可用性判定收进 `src/lib/dropCapabilities.ts`，两种语言、两条投递路径共用一条"能处理就处理，不能处理就禁止"的规则。同一节里另有两处：内部拖拽被原生 `enter` 报成"没有文件"、从而三个按钮全部点亮（`dragPathsKnown()` 改为合并），以及禁止态定为**按钮禁用＋光标禁止符号**——光标原本由 wry 固定回报 Copy、页面改不动，现在由 `src-tauri/src/drop_cursor.rs` 在运行时接管那两条回报方法，把页面的判定翻译成原生拖放的拒绝（同一节也记了这条路上"被拒绝的手势没有收尾事件"这个坑）。局域网接收的文件从此也受「完成后在访达中显示」控制，一批只在最后一个落地时显示一次；「运行组件」缺失多项时可以一次装完。 |
| 0.1.13 | Windows 实测「运行组件」一键安装失败：`winget` 把“已经装过”报成非零退出码，界面就照着说安装失败（见「Windows 一键安装运行组件」）。同一节还记了 `--source` 未钉住导致的静默挂死、机器级组件必须提权、以及装完之后进程内 PATH 仍是旧值这三处。另有三处与系统资源相关的约束：托盘改用自绘的黑白模板图（`scripts/build-icons.py` 重建，应用图标同一份几何，`icon_as_template` 只在 macOS 生效）；钉图窗口的放大尺寸与位置必须按 work area 与 DPI 换算后夹取，见「钉图窗口的放大与还原」；闲置时的取样节奏——伴生进程 `native/macos/drag-bridge.swift` 无按压时 10Hz、Windows 手势监视 `monitor_pause` 在圆环关闭时 250ms、发现线程静默时 1.6s——都要在“关掉了就别一直占 CPU”和“还听得见、还能过期”之间取平衡。另有两条：主面板顶部的升级提示与托盘那一行共用同一份状态，而这份状态必须落到磁盘上，否则探测一天只跑一次，用户在探测当天之后就再也看不到提示——包括升级重启后的那一次启动；落盘的提示在恢复时要重新判定它是否还算数，见「升级提示跨启动存活」。 |

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
覆盖真实跨系统发现；CI workflow 能验证协议单元测试，但没有双机网络夹具，因此发布前
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

## 「运行组件」可以一次装多个缺失项

面板原先一次只能装一个组件：`runtimeInstalling` 是一个 `string | null`，`install` 命令
只接受一个 `component_id`，任何一个按钮在忙时就把整片按钮全部禁用。缺失 Pandoc 与 FFmpeg
时用户要点两次、等两轮重新检测。

现在前端把缺失且可安装的组件收成一个列表，标题栏在**缺失不止一项**时给出「一键安装
N 个组件」，行内的单个安装按钮保持原样，两种入口调用同一个 `install_runtime_components`。

| 约束 | 原因 |
| --- | --- |
| 后端逐个串行安装，不能并发 | `brew` 与 `winget` 同一时刻只允许一个安装，并发只会互相锁住并报错。 |
| 同一个 `package_id` 只装一次 | Windows 上 `pdf-render` 与 `pdf-to-word` 都指向 Poppler，按行安装会重复下载同一个包。 |
| 只有“包管理器本身坏了”才中止整批 | 没装 brew / winget、没有源、网络不通、winget 不回应：剩下的行只会把同一个错误重复 N 遍、各等一次超时。但**单个包**的失败（用户关掉 UAC、某一个安装程序自己拒绝）必须继续往下装，否则「一键安装 N 个组件」会因为一个组件被拒绝而放弃其余四个。判定收在 `ends_the_batch()`，有测试钉住。 |
| 无论成功失败都 `refreshRuntime()` | 中止的批次里先前的安装可能已经落盘，面板必须重新检测才能显示真实状态，不能相信这一个回复。 |
| 安装结果的名字由界面措辞 | `install_runtime_components` 返回 `()`：按既有约定，后端只报代号，`runtimeInstallComplete` / `runtimeInstallManyComplete` 两种数量由界面选择。 |

## Windows 一键安装运行组件

### 0.1.13 之前实测必然失败

用户在 Windows 机器上点「一键安装组件」报失败。原先的实现是一条
`winget install --id X --exact --silent --accept-package-agreements --accept-source-agreements`，
非零就报错，然后用 `resolve_tool()` 验证二进制在不在。四个独立原因都藏在这十行里：

1. **`winget` 把"已经装过"报成非零退出码。** `0x8A15002B`（PACKAGE_ALREADY_INSTALLED）与
   `0x8A150016`（没有可升级版本）都是成功语义，但 `ExitStatus::success()` 一律当真失败。
   机器上已经有 Pandoc 的用户，越"没问题"越必然看到"安装失败"。
2. **不钉 `--source`。** 同时挂了 msstore 源的机器上，未钉源的 `winget` 可能停在一条
   `--silent` 永远不会回答的源协议上；表现就是"卡住不动"，最后由 15 分钟超时收成失败。
3. **机器级组件必然要提权。** Poppler / FFmpeg / ImageMagick / Typst 默认装到
   Program Files，需要管理员；Shift 自身是免管理员的 GUI 进程，`--silent` 拿不到 UAC 就
   直接失败。原先没有任何提权路径。
4. **装完之后再验证也找不到。** 进程只持有启动时的环境块：`winget` 写进注册表的
   `%LOCALAPPDATA%\Microsoft\WinGet\Links` 不会回流到运行中的 Shift，`which` 于是仍按旧
   PATH 回答，`winget-install-missing-binary` 把一次真正成功的安装报成失败。

### 不能随意改掉的约束

| 约束 | 原因 |
| --- | --- |
| 退出码先判，代码表认不出来时才看 `winget` 的措辞 | 退出码是精确信号；措辞只用来分家两件码里分不开的事（用户关掉了 UAC、源没有接受协议）。反过来让措辞压过码，一句无用的日志就能把成功改成重试。 |
| 只按完整码匹配，不按低 16 位 | Win32 的 `0x8007_xxxx` 与 WinGet 的 `0x8A15_xxxx` 会撞低 16 位；只看尾巴会把普通的访问被拒读成"需要提权"。 |
| 每条命令都带 `--source winget` | 去掉它就会在挂了 msstore 的机器上静默挂死到超时——而那看起来就是"安装失败"。 |
| 阶梯：不指定 scope → `--scope user` → 提权 `--scope machine` | 第一档就是历来发出去的那条命令，默认装到用户目录的包行为完全不变；`--scope user` 是"不弹 UAC 也能装机器级包"的出口，且 WinGet 在选安装器阶段就会拒绝不支持的 scope，所以这一档只花一次握手而不是整包下载；UAC 是最后一招，因为一个本该是开关的操作弹出授权框，正是"看起来坏了"的来源。 |
| 提权走 `powershell Start-Process -Verb RunAs -Wait -PassThru` | 无控制台的 GUI 进程自己弹不出 UAC；提权后的 `winget` 写在自己的控制台里，但**宿主** shell 的输出仍在我们手上，而"用户取消"只在这里以文字出现（1223 只是旁证）。 |
| 成功与否最终由磁盘上有没有那个可执行文件决定 | 装完先 `refresh_process_path()` + `reset_tool_cache()` 再找。`RegGetValueW` 不加 `RRF_NOEXPAND`，注册表里的 `REG_EXPAND_SZ` 会像系统构造进程环境那样展开。找不到才说"没装上"，并且说成"重启 Shift 后重新检测"，而不是"安装失败"。 |
| 单个包失败不许放弃整批 | 见上一节；判定在 `ends_the_batch()`。 |

`src-tauri/src/process.rs` 里 `output_hidden_with_timeout()` 是这套逻辑唯一的输出通道：
它同时保留 `CREATE_NO_WINDOW`（GUI 进程不闪控制台）与持续排空管道（否则子进程写满管道会
一直堵到超时）。超时消息统一含 `TIMEOUT_MARKER`，调用方据此区分"没回答"与"回答不要"。

### 回归测试

```bash
cd src-tauri
cargo test --lib runtime
```

- `an_already_installed_package_is_not_a_failure`：钉住原因 1，也就是这次实测失败的主因。
- `exit_codes_are_separated_by_who_can_fix_them`：谁能修就分给谁，并钉住低 16 位撞车。
- `every_winget_attempt_pins_the_community_source`：三档命令都必须带 `--source winget`。
- `the_ladder_escalates_scope_only_when_it_must`：未提权的两档都不许要求 machine scope。
- `the_installer_own_words_break_a_tie` / `a_closed_prompt_reads_as_a_refusal`：措辞只在码
  认不出来时生效，且"网络断了"不能被读成"用户说了不要"。
- `only_a_broken_package_manager_ends_the_batch`：整批中止的判定。

## 钉图窗口的放大与还原

钉图窗口原先只有「复制」和「关闭」，用户要的「放大」按钮并不存在——不是坏了，是没有。
现在一颗按钮管两个方向：放大到屏幕，或退回钉住时的大小。

| 约束 | 原因 |
| --- | --- |
| 尺寸按 work area，不按整块屏幕 | `monitor.size()` 含菜单栏与 Dock。按整屏算出来的钉图会有一部分永远压在菜单栏底下，而无边框窗口拖不回自己身上。 |
| 逻辑像素给 `set_size`，物理像素给 `set_position` | 两者单位不同。混用后在 2x 屏上放大就是两倍大、位置偏一半——这台机器上验得出来，在 1x 外接屏上看不出来，所以由 `PinArea::logical_size()` 显式换算并命名。 |
| 先 `set_size` 再 `set_position` | 混合 DPI 的桌面上，改尺寸可能把窗口挪到另一块屏，此时窗口自己的 scale factor 已经变了；顺序反过来位置就按旧的 scale 算。 |
| 放大幅度封顶 3 倍 | 预览最长边本来就是 1040 px（`PIN_PREVIEW_EDGE`）。再往上只是把锯齿摊开，用户看到的是糊成一片的马赛克，不是更大的字。 |
| 放大以窗口中心为锚点，并夹在 work area 内 | 以左上角为锚点读起来像"窗口被搬走了"；不夹取的话，贴着屏幕右缘钉住的图一放大就整张跑到屏幕外。 |
| 每次重新钉都回到小图 | 窗口是复用而不是重建（`PIN_ENLARGED` 是唯一的记录）。否则上一次放到最大的用户，接下来每一张钉图都糊满全屏。 |
| 读不到显示器就答小图，不猜 | `pin_window_size(.., None, true)` 走的是小图那一支。按一块幻影屏幕算出来的尺寸，是把图放到用户够不着的地方。 |

按钮的文案（放大 / 恢复原大小）跟的是页面自己那份 `enlarged`，而真实尺寸只有 Rust 知道；
`zoom_pin` 把翻转后的状态返回给页面，两边才不会各说各话。

### 回归测试

```bash
cd src-tauri && cargo test --lib editor
```

`a_fresh_pin_never_exceeds_its_own_picture_or_520_px`、`enlarging_fills_the_screen_but_stays_inside_it`、
`a_small_picture_is_not_blown_up_to_the_ceiling`、`an_enlarged_pin_grows_from_its_centre`、
`a_pin_against_a_screen_edge_grows_inward_not_off_screen`。后两条钉的是"从中心放大"和
"贴边时向内长"，因为它们在 1x 的单屏开发机上看起来永远是对的。

## 闲置时的取样节奏

Shift 绝大部分时间是停在菜单栏里没人碰的。这一节的每条节奏都是从实测里挑出来的，
目的是把"关掉了就别一直占 CPU"和"还听得见、还能过期"同时满足。

### 实测的 A/B

在装有 0.1.11 的机器上，两个版本各自空转 60 秒，按进程分别读 `ps -o time=`：

| 进程（60 秒空转） | 0.1.11 | 0.1.13 | 变化 |
| --- | --- | --- | --- |
| `drag-bridge` 伴生进程 | 140 ms | 40 ms | **−71%** |
| `shift` 主进程 | 100 ms | 60 ms | −40% |
| 合计 | 240 ms | 100 ms | **−58%** |

只有伴生进程那一行可以直接归因：它没有窗口，所以不受"主窗口有没有开着"影响，
而其余三处改动都发生在那两个数字里。主进程那一行不能当成"发现线程省了 40%"来读——
同一个构建重复测量的摆动就有 ±40ms，和差值同量级。发布前若要再引用一次，
请重新测，不要抄这张表。


| 位置 | 节奏 | 原因 |
| --- | --- | --- |
| `native/macos/drag-bridge.swift` | 无按压 10Hz，按住切 30Hz | 伴生进程是**另一个被跟踪的进程**，原先永远 30Hz，而且每跳都分配一个 CGEvent 去读一个没人要的鼠标位置。它是这一节里唯一能被干净归因的改动：伴生进程没有窗口，所以它的 CPU 与「主窗口是否开着」无关。`NSTimer` 不能改自己的间隔，所以是换掉计时器而不是调它。 |
| `src-tauri/src/drag_bridge.rs` `monitor_pause`（Windows） | 圆环开着 33ms，关了 250ms | 圆环开着时必须 33ms：这条循环要靠观察到"按下"才能从按下点量起拖拽阈值，放慢不是"晚一点发现"，而是**根本记不住按下**，于是快速拖动的用户从此开不了环。只有关掉圆环时才敢放慢，那时要等的只有开关本身。 |
| `src-tauri/src/share.rs` 发现线程 | 信标 2s / 扫段 30s 不动；静默时收包等待 400ms→1.6s，无 peer 时账目检查 600ms→2.4s | **关掉 ShiftDrop 不等于变聋**：字段注释写的是"不主动播报"，`recv_from` 从来不受 `enabled` 约束，所以一台关掉的机器仍然听得见、仍然能被别人选来发送；`expire()` 也必须继续跑，否则一小时前的 peer 会永远挂在列表里冒充在线。所以省的是唤醒次数，不是探测能力。播报与扫段的频率一点没动，peer 被发现的速度因此不变。 |
| `src/components/MainWindow.tsx` 30 秒时钟 | 页面可见才跑 | 主窗口一生大部分时间是隐藏的；隐藏的页面里每 30 秒重绘一次没人看的列表，正是 nex_squid `useSystemSnapshot` 用 `visibilitychange` 停掉的那类开销。空列表连 `setNowTick` 都省了——重新计时的是不存在的行。 |
| 最近列表缩略图 | `IntersectionObserver` 只保留在视线内的图片 | `loading="lazy"` 只**推迟**解码，从不**释放**。在 WKWebView 里用 250 张真实截图量过：静止时 13 张解码，滚到底单调涨到 136 张再也不降；观察者接管后同样的滚动稳定在约 20 张。root 必须是列表本身而不是视口——行的裁剪来自列表的 `overflow-y`。 |

托盘图标也在这一节，因为它省的是同一个东西：`src-tauri/icons/tray-template.png` 是
`scripts/build-icons.py` 画的纯 alpha 黑白标记（中性灰只在 Windows 上当底色用，
macOS 完全忽略颜色），`icon_as_template(cfg!(target_os = "macos"))` 只在 macOS 生效——
Windows 没有可以请求的着色。**别把它换回 `app.default_window_icon()`**：那是彩色转盘，
`tray-icon` 会把它按 18pt 高等比缩放，在菜单栏里就是一团色块，这正是"系统图标该是
黑白"这条反馈的来由。

## 升级提示跨启动存活

主面板顶部要显示升级信息，点一下跳到「设置 → 软件更新」。界面这一半很直接：
`useUpdateFlow` 在 MainWindow 里再挂一份，它只**读** Rust 记着的状态并订阅
`update://status`，不会自己发探测请求，所以两处视图不会各自查网络，也不会给出
对方已经做完的动作。

难的是另一半：探测**一天只允许跑一次**（`update_check_due` 看 `last_update_check_day`），
而原先 `UpdateState` 是纯内存的。两者一叠，"发现新版本"只会在真正跑探测的那一天出现，
之后就再也见不到——**尤其是升级装完、应用重启的那一次启动**，恰恰是用户最该看到的时候。
实测就是这么撞上的：第一次启动看不到横幅，把时间戳改老才出现。

| 约束 | 原因 |
| --- | --- |
| 提示要落盘，存在 `app_data_dir/update-notice.json` | 和 `history.json` 同一目录、同样在安装包之外，因此升级和重装都还在。只有 `available`/`ready` 值得留；其他结果一律**删掉**文件，于是一次失败的检查不会把旧结论留在面板上冒充新闻。 |
| 恢复时重新判定它还算不算数 | 版本已经不领先就整条丢掉——用户已经装上那个版本了，留着就是横幅 forever 地推荐他正在跑的版本；这也是清理的唯一时机，`revive_notice` 返回 `None` 时顺手删文件。 |
| `ready` 只有在安装包文件真在磁盘上才保留 | "已下载并校验"承诺的是临时目录里的一个文件，系统完全可以在 Shift 关着的时候清掉它。文件没了就降级成 `available`：这句话仍然为真，只是要重新下载一次。 |
| 恢复发生在 `setup_tray` **之前** | 托盘那一行（`update::tray_title`）读的是同一份状态。放在后面，横幅回来了而托盘行没有，两个入口自相矛盾。 |
| 关掉"每天检查更新"就不恢复 | 这个开关的语义是"别告诉我"。留着提示等于用磁盘缓存绕过用户刚刚做的决定。 |
| 用户点"忽略"要连磁盘那份一起删 | 否则"忽略"只有效到下次启动，横幅天天回来。 |

### 回归测试

```bash
cd src-tauri && cargo test --lib update::
```

`a_notice_for_a_release_still_ahead_survives_the_launch`（并且钉住恢复出来的
`current_version` 必须是**现在**在跑的版本，不是当初问的那个）、
`an_installed_release_stops_being_announced`、
`a_ready_notice_without_its_installer_downgrades_rather_than_lying`。

## 所有图标共用一份几何

同一句反馈的后半段是"应用图标要和当前的单色图标保持一致设计"。这句话的重点在**设计**，
不在**单色**：托盘那张必须是黑白的，因为它由系统着色；应用图标该是彩色的，因为 Dock
瓦片背后没有任何东西给它着色。真正让两张图标成为同一个作品的，是几何和填充规则，不是
颜色。所以两者由 `scripts/build-icons.py` 里的同一组常数画出来
（`OUTER`/`INNER`/`HUB`/`GAP`/`PLUS_ARM`/`PLUS_BAR`），只有背后允许出现的东西不同。

| 约束 | 原因 |
| --- | --- |
| 一致性来自几何与**平涂**，不来自配色 | 六块扇形、同样的扇区间隙角、同样的内外环与 hub 半径、加号同样是从 hub 里**挖**出来的负空间，以及同样一律平涂。旧图标之所以像另一个产品，不是因为它有颜色，而是因为它是一整块橙紫渐变底 + 径向高光 + 投影 + 六块扇形各配一种奶油色——那是另一套视觉语言。 |
| 应用图标最多三色，且不得出现渐变/高光/投影/逐扇形配色 | 加了第三色（hub 用 `#f04f24`）仍然守得住，因为它是平涂；一旦加回渐变就立刻回到旧语言。色相全部取自界面本身：`#FFB25C` 是旧母版本来就带的暖色，`#f04f24` 是界面里每个按钮的强调色。 |
| hub 深、外环浅的**朝向**不要反过来 | 被替换的那版就是"暖色 hub 在更浅的圆盘里"。Dock 图标换代时把内外反转，老用户会觉得图标被换掉了，而不只是被重画了。 |
| 环形几何只在 `scripts/build-icons.py` 定义一次 | 托盘那张 PNG 是用它逐字节重绘出来的（改完脚本跑一遍，产物与仓库里的文件 `cmp` 相同）。几何分两处写就会漂移，而漂移只在 16px 下看得出来。 |
| SVG 母版里的瓦片是**内缩**的（820/1024） | `tauri icon` 只会等比缩放母版，**不会**加边距。母版画成满幅，产出的 `icon.png` 就是满幅方块，而仓库里原有的整组图标是 410/512＝0.801 的圆角瓦片。 |
| 轮盘直径取瓦片的 0.795 | 被替换的那版母版是 r=326 画在 820 的瓦片里（0.795）。照抄这个比例，新图标在 Dock 里的视觉重量和旧的一致，不会突然变大。 |
| 加号用瓦片的颜色画，不真的打穿成透明 | 在 Dock 上两种做法看起来完全一样；但 `tauri icon` 合成 iOS/Android 图层时，真的洞会被平台背景填掉。 |
| 用 `--tauri` 重生成整组，而不是手搓 | 实测 `tauri icon` 产出的 `icon.icns` 条目（ic07…ic14 + is32/il32/s8mk/l8mk）与 `icon.ico` 尺寸梯（16/24/32/48/64/256）跟仓库里已有的**完全一致**，且它不会碰 `tray-template.png`。 |

验收看 16px：16px 时轮盘只有约 10px，六条扇区间隙约 0.46px、加号约 0.38px，都已经是
亚像素——被替换的那版在 16px 的加号也只有 0.72px，所以这不是退步，而是这套几何在小尺寸
下的固有结果。真正必须在 16px 还读得出来的是**外环的扇形分割**和瓦片轮廓，深浅两种桌面
下都验过。hub 的 `#f04f24` 要到 24px 才出现像素、48px 才站稳，这是有意的。

`public/shift-icon.png` 是 `icon.png` 的副本，不是另一张图：它在主窗口与编辑器标题栏里以
26 CSS px 显示（Retina 上 52 设备 px），那个尺寸下渐变版的中心已经糊成一团，而平涂版的
扇形分割和 hub 还分得开。

## 「最近使用」拖回主面板的三个按钮

0.1.8 曾经把这条交互关掉：一行拖出去是文件，拖回来却什么都不是，用户只能重新选一次文件。
现在拖到「文件转换 / 文件工具 / 文字识别」上就执行该按钮的动作。

**按钮是唯一的落点，也是唯一会响应拖放的东西。** 这条规则分两步收紧：

- 0.1.9 去掉了"落在面板空白处就打开操作环"的兜底，改成不执行操作、弹一条提示。
- 0.1.10 连提示也去掉：既然不执行任何操作，解释"为什么没执行"的浮层只是在用户没有松手的位置
  制造动静，落在面板其他位置现在是彻底静默。

「文字识别」的拒绝方式在同一步收紧里改成了**事前可见**：0.1.10 用提示说明"这个格式识别不了"，
但那仍是让用户先做错、再由浮层告知。现在拖拽一开始就按携带的文件判定，识别不了的按钮直接进入
不可用态，松手时什么都不发生，也不再有任何提示。

**不可用态写成"禁用 + 光标禁止符号"，不额外画一层符号。** 拒绝要落在光标上：鼠标底下那个
复制角标本来就是 AppKit 对"这个落点要不要这个文件"的回答，能服务就画 +，不能服务就画禁止，
用户不需要先学会读按钮的配色。0.1.11 中途曾把拒绝画成按钮上层的禁止符号，理由是**页内拖拽改得动
`dropEffect`，外部文件却改不动光标**：外部文件走原生拖放通道，而 **macOS 下 wry 的
`draggingEntered:` / `draggingUpdated:` 固定回报 `NSDragOperation::Copy`**
（`wry/src/wkwebview/drag_drop.rs`，它甚至会把 super 返回的 `None` 改写成 `Copy`）。
现在的结论是：这条路能接管，于是拒绝交还给光标，按钮只保留 `disabled` 与配色弱化，不再叠一层符号。

**`src-tauri/src/drop_cursor.rs` 只改"回答"，不改"投递"。** 页面是唯一知道"拖的是什么、
指针停在哪个按钮上"的东西，所以它把判定用 `set_drop_refusal` 命令交下来；原生侧在运行时替换 wry
那个私有类的两条回报方法（类名由 `wry::wkwebview::class::wry_web_view::WryWebView` 前缀扫描得到，
因为真名带 crate 版本，写死会在升级时静默失效），拒绝时把 `Copy` 收窄成 `NSDragOperationNone`。
**两次都必须先调用 wry 自己的实现再改它的返回值**：那次调用正是 `over` / `dragover` 的投递入口，
拒绝分支若直接 `return`，页面就再也收不到指针移动，一次拒绝会粘到整次手势结束——从不能服务的按钮
拖到能服务的按钮也不会亮。Windows 不需要这套：OLE 每次移动都会来问本进程，回报跟随页面设的
`dropEffect`，光标本来就会是禁止。

**被拒绝的手势没有收尾事件，所以盯鼠标松开。** 落点回报 `None` 时 AppKit 既不发
`performDragOperation:` 也不发 `draggingExited:`，按钮会一直停在禁用态、面板看起来卡住了。
`drop_cursor.rs` 因此在拒绝生效期间起一个线程轮询 `CGEventSourceButtonState`，松手时向主窗口
发 `drop://cancelled`，页面用它收起 `dragging` / `dropAction` / `dragPaths`。

**每次手势只报一次判定，起点必须是"欢迎"。** `draggingEntered:` 会把判定复位成不拒绝——文件还
没认全就拒绝会废掉一次本来能用的拖拽。而页面是在 `enter` 之后才回答的，所以 `useEffect` 的依赖里
必须带上一个"第几次拖拽"的计数器（`dragSession`），否则状态没变化的第二次拖拽不会重新上报，
上一次留下的判定会一直生效。

**按钮变暗用自身配色，不用全应用的 `opacity: var(--dimmed)`。** 不用 `opacity` 最早是为了
保住画在按钮之上的禁止符号——符号不能跟着被调淡。符号换成光标之后这条理由已经不在了，配色
没有再动：整颗按钮调淡会和"还在加载"混在一起，而改用 `color` / `background` 弱化读起来就是
"这颗关掉了"。留着它也省下一次已经验收过的观感改动。

**`is-drop-unavailable` 必须 `pointer-events: none`。** 落点由坐标命中（`actionAtPoint`）而不是
由"哪个元素回答了事件"决定；禁用按钮若仍占住命中区，`disabled` 后的按钮不再回应拖拽事件，
手势会停在最后一个应答过的按钮上。把它让开，命中判定与视觉状态才可能一致，顺带让 hover 反馈
彻底无从发生。

`dragPaths` 是判定唯一依据：原生路径在 `enter` 拿到 `paths`，内部路径在 `dragstart` 写入这一行
——`over` 事件不带路径，`dataTransfer` 在 `dragover` 里也读不到，所以两条路都必须在开始时登记。

### 内部拖拽会被原生 `enter` 报成"没有文件"

0.1.11 实测最隐蔽的一个 bug：把「最近使用」的一行拖到按钮上，**三个按钮全部点亮为可处理**，
松手却什么都不做——正是用户描述的"反馈为可处理文件，但又不给出操作"。原因是 macOS 上一次内部
拖拽会被**两条通道同时上报**，而原生 `enter` 的载荷里没有文件名：wry 的 `collect_paths` 只读
`NSFilenamesPboardType`，页内拖拽携带的是 `text/uri-list`，于是回报 `paths: []`。这个空数组把
`dragstart` 已经登记的 `[行路径]` 覆盖掉，而 `actionCanServe` 按既定规则必须把"文件未知"当作
不禁止，三个按钮就全成了候选落点。

修法是合并而不是覆盖：`dragPathsKnown(known, reported)`（`src/lib/dropCapabilities.ts`）——
报告非空时以报告为准，报告为空时保留已知的那一份。**不能反过来"信任最新一条"**：两条通道谁先到
因平台与 WebView 版本而异，覆盖语义必然在其中一种顺序下失效。也**不能把空数组直接判成"全禁用"**：
那是 0.1.10 之前 Windows 拖拽整片灰掉的成因（见下条 `actionCanServe`）。

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
| 三个按钮共用 `src/lib/dropCapabilities.ts` 一条规则 | 判定原先只写在「文字识别」上，另外两个按钮照旧执行，于是"能处理就处理、不能处理就禁止"在同一个面板里有两种口径。视觉状态（`spent()`）、`dropFiles` 的实际执行、`aimedAction` 的点亮，三处必须问同一个函数，否则会出现"看着能按、按下去什么都不做"或者反过来。 |
| 「处理不了」不接受兜底，也不解释拒绝 | 按钮说做什么就做什么：既不能转交别的圆环，也不能弹一条"为什么不行"——那等于让用户先做错再被纠正。按钮禁用加光标禁止符号就是全部答复。 |
| 拒绝交给光标，按钮侧不再叠符号；为此必须接管原生回报 | 光标上的复制角标本来就是"这个落点要不要文件"的答复，两条投递路径都能改到它，观感天然一致；代价是 macOS 要在 `drop_cursor.rs` 里改掉 wry 固定回报的 `NSDragOperation::Copy`。页内拖拽仍走 `dropEffectFor()`，Chromium 会照它画禁止光标。 |
| 禁用按钮要 `pointer-events: none` | 落点靠坐标命中，不靠元素应答；禁用后按钮若仍占命中区，拖拽事件不再回到它身上，`dropAction` 会停在上一个应答的按钮上。 |
| 内部拖拽的原生 `enter` 不带文件名，要合并不要覆盖 | `collect_paths` 只读文件列表，页内拖拽带的是 `text/uri-list`，于是回报空数组；直接写入会把 `dragstart` 登记的行路径冲掉，三个按钮全点亮、松手又静默。`dragPathsKnown()` 保留已知的那一份。 |
| `actionCanServe` 在"文件未知"时必须返回 true | Windows 的拖拽在载荷可读之前可能报告 0 个文件；按空列表判定会把三个按钮全部禁掉，一次本来能用的手势就废了。只有"知道文件、且知道处理不了"才禁止。 |
| 「文件工具」永不禁止 | 它的环以打开 / 在文件夹中显示 / 发送收尾，任何存在的文件都能执行；把"其他"类型判成处理不了会让面板出现三个灰按钮。 |
| 拖出的行落在窗口之外时不能命中按钮 | `dragend` 的坐标也要经过 `actionAtPoint`，而命中判定四周有 `SLACK_X/SLACK_Y` 余量；不先判窗口边界，拖去资源管理器的一行会被当成落在按钮上。 |
| 拖拽结束时三处都要清空 `dragPaths` | 原生 `leave`、原生 `drop`、行的 `dragEnd` 都要清；`landRecentRow` 也清一次，因为部分引擎在落点已确定后才补 `dragEnd`，只靠它会让按钮在下一笔拖拽前一直显示为不可用。 |

### Windows：一次内部拖拽可能一次都不报告

0.1.10 用户实测：Windows 上把「最近使用」的行拖到三个按钮，**每一个都完不成**。台账
里"只保留 DOM，Windows 的原生投递会让拖拽失效"写反了方向——在 Windows 上两条路都可能不到：

1. **DOM 路径**：历史行是 `draggable` 的文件式拖拽，WebView2 在自己的窗口上注册了 OLE
   `IDropTarget`（`wry/src/webview2/drag_drop.rs`），页面内的 drop 被它接管。
2. **原生路径**：那个 `IDropTarget::DragEnter` 只读 `CF_HDROP`；页内拖拽不带这个格式，
   于是 `enter_is_valid` 保持 false，`DragOver` / `Drop` / `DragLeave` 一律不上报。

两条都静默，松手就没有任何事件，于是"什么都会发生"被用户读成"拒绝"。修法是让 `dragend`
成为兜底：它总会上报，且带 `clientX/clientY`，足够命中按钮；`landed` 标记保证它只在两条
投递路径都没到过的时候执行一次。

**0.1.11 第一次修错了方向。** 那版把兜底写在 `dragEnd` 里立刻执行，而 macOS 的原生
`drop` 是在按钮抬起**之后**才到的：兜底先用 `dragend` 自己的坐标判定（Chromium 在这里报
的是 (0, 0)，不是松手位置），把 `landed` 置真，随后真正的那次 drop 被当作"这一笔已经完了"
丢掉——结果从"偶尔不响应"变成"三个按钮全部无反馈"。现在兜底满足两条：

| 约束 | 原因 |
| --- | --- |
| 兜底必须延迟到真正 drop 之后（260 ms），且只由 `landed` 决定谁执行 | 谁先谁是正确答案因引擎而异；macOS 是 drop 在后，页内拖拽在 Chromium 是只有 `dragend`。 |
| 兜底只在**命中按钮**时才落地 | 没命中时它给不出任何 0.1.10 给不了的东西，却可能抢在真正的 drop 之前把这一笔标记完成，把它吃掉。 |
| `dragend` 坐标为 (0, 0) 时改用在途最后一次 `dragover` 的位置 | (0, 0) 是"没报告"而不是"在左上角松手"；Chromium 就是这样。 |
| 不要改成"记住离开窗口时就丢掉该位置" | Chromium 在 `dragenter` / `dragleave` 上把 `relatedTarget` 固定为 null，按它清位会让 Windows 的兜底永远拿不到落点——又回到不响应。剩下的误判只剩"从按钮上拖出去"这一种路径，代价远小于什么都不做。 |

### 拖动时的图像是文件，不是那一行

不加 `setDragImage` 时，引擎拍的是指针下的元素本身：缩略图 + 文件名 + 时间 + 目录 + 五个悬停
按钮，看上去像在拖整个列表。`src/lib/dragImage.ts` 造一个和圆环拖拽示意同款的"一个文件 +
名字"的图（图片行复用列表里已经解码好的缩略图，其他行显示扩展名）。它挂在 body 外、屏幕外，
250 ms 后自删：两个引擎拍照的时机不同帧，删早了就拍不到。

### Windows：闲置的圆环会压在按钮上

圆环窗口无边框、`always_on_top`，用完不关就停在原地等点击，而它可能正压在面板的按钮上。
开始拖动一行时先调用 `stand_down_radial` 让它让开；`radial_dragging` 为真说明这次圆环属于
一个正在进行的 Shift 拖拽手势，那时不能收——结束手势的松手才有权处置它。

注意不要把它改成"圆环在 `enter` 时自行让开"：往打开的圆环上直接放文件、由落点扇区执行该
动作，是 Windows 现有的一条可用路径，被闲置判定挡掉就是功能倒退。

### 回归测试

`src/lib/dropTargets.test.ts` 覆盖命中区域与坐标换算，`src/lib/dropCapabilities.test.ts`
覆盖"谁能服务这次拖拽"这条规则（含类型表与后端一致、PDF 不落到 OCR、载荷未知时不禁止）；
拖拽本身用浏览器实测（Chromium 与 WKWebView 两条路径都要走一遍）：

1. 「最近使用」某行 → 拖到「文件转换」→ 只弹出一个格式环，且文件是这一行。
2. 同一行 → 拖到「文件工具」→ 只弹出工具环。
3. 图片行 → 拖到「文字识别」→ 按钮正常点亮，光标带 +，打开编辑器并自动开始识别；非图片行 → 「文字识别」拖拽全程为禁用态、**光标为禁止符号**、不会点亮，**hover 完全无反馈**，松手后不打开编辑器、不开环、**不出现任何提示**，另外两个按钮仍可正常落点。**同一条非图片行还要从访达再拖一次**：两条通道的光标观感必须一致（这是 0.1.11 的主要回归点）。
3b. `.key` / `.zip` 这类没有可转换目标的行 → 「文件转换」同样全程禁用＋光标禁止、松手不执行；「文件工具」始终可用并正常弹出工具环。
3c. **Windows 必须单独跑一遍 1–3**：这一平台两条投递路径都可能不上报，兜底只在 `dragEnd` 里，macOS 通过不代表 Windows 通过。
3d. 先用「文件转换」弹出一个圆环、再拖一行到按钮上 → 圆环不能吃掉这次拖放（Windows 尤其要看这一条）。
3e. 拒绝之后把文件拖到**能服务的按钮**上 → 光标必须重新变成 +、按钮重新点亮、松手正常执行。原生侧一旦在拒绝时提前返回而跳过 wry 的实现，页面就收不到指针移动，整段手势都会停在拒绝上。
3f. 连续拖两次同一批文件 → 第二次不能残留第一次的判定（`dragSession` 就是为了让每次手势都重新上报一次）。
4. 同一行 → 拖到面板空白处 / 标题栏 / 列表本身 → 不开环、不执行、**不出现任何提示浮层**。
5. 从访达拖文件到面板空白处 → 同样不执行、不提示；拖到任一按钮 → 执行该按钮的动作。
6. 行拖出到访达/聊天窗口 → 仍是文件本体，没有被 Shift 自己吃掉。
7. 两种拖法各拖一次 → 面板（`.home-drop`）在拖拽全程不缩放、不变色，标题文案不变；只有三个按钮在拖拽开始时同时亮起虚线候选态、松开后熄灭。
8. 松手之后（无论落在哪里）→ 按钮立刻恢复正常亮度，下一次拖拽不会残留不可用态。

## 「完成后在访达中显示」管到的两条落地路径

`auto_reveal` 这个开关原本只有转换完成时会读：`perform_file_action` 结束前判断一次，
弹出第一个输出。局域网收到的文件走的是 `share.rs::receive_file`，它自己 `emit`
`app://file-result` 并写入历史，从来不经过那条判断，所以「接收完就弹一个访达窗口」
不受任何设置控制。

现在两条落地路径共用同一条规则：**文件落地之后是否把文件管理器带到前台，由
`settings::auto_reveal` 决定**。批量接收沿用批处理的既有语义——一批只在最后一个文件
落地时显示一次（`reveals_on_landing`），而不是每个文件弹一次窗口，与一次转换十个文件
只弹一个输出的行为对齐。

| 约束 | 原因 |
| --- | --- |
| 接收路径必须读 `auto_reveal`，不能自己决定弹窗 | 同一件事有两种行为，用户只能理解为开关坏了；转换那侧已经是这个口径。 |
| 一批文件只按最后一个文件显示一次 | 显示的是文件管理器窗口，十个文件弹十次等于抢占前台十次。 |
| 批内序号必须在 `emit_transfer` 之前读 | 那条事件会把 `header.peer_name` 移出，之后再借用就是编译错误；先算成一个 `bool` 才能既保住顺序又保住借用检查。 |
| 缺字段的旧版本信标必须仍然显示 | `batch_index` / `batch_total` 都取默认 0 时，`index + 1 >= total` 成立，`saturating_add` 防的是 0 - 1 下溢。 |

### 回归测试

`cargo test --lib share` 的 `only_the_last_file_of_a_batch_reveals`：钉住单文件、批次中间、
批次末尾与旧版本信标四种头部的判定。

## 提示语由界面措辞，后端只报代号

同一件事在两个地方各说一遍，就一定会说成两样。0.1.10 之前正是这样：`perform_action` 直接返回
`"Saved 1 file."` 这类英文句子，界面在中文环境里也就照原样显示，而同一个环里别的结果用的是
`t("saved")`；写目录被拒的提示同样由 Rust 拼成英文长句，macOS 首次运行必然看到它。

现在约定只有一条：**后端说"是哪件事"，界面说"这句话怎么说"。**

- 结果：`ActionOutcome.message` 只能是 `outcome*` 代号，由 `src/lib/feedback.ts` 的
  `outcomeMessage()` 措辞；数量取自 `outputs.length`，不再把数字写进句子再让人解析。
- 失败：`Err("…")` 用 `err*` 或 `share-*` / `homebrew-*` 这类既有小写代号；显示前统一走
  `t(messageFrom(error))`。`translate()` 查不到 key 时原样返回，所以第三方库抛出的原始句子
  （`ffmpeg exited with status 137`）仍会照实显示，不会被压成一句无信息的话。
- 措辞查不到就不要显示代号：`outcomeMessage()` 对未知 `outcome*` 回落到 `saved`。

`src/lib/feedback.test.ts` 会扫过 `src-tauri/src/*.rs`，要求每一个 `err*` / `outcome*` 代号在
两种语言里都有措辞，且 `outcome*` 必须被 `feedback.ts` 认识。新增代号时不必改测试——它会自己发现漏掉的
那一个；删掉一个还在用的代号同样会红。

另一条容易踩的约束：**一次性注册的监听器不要闭包捕获 `t`。** 挂载时注册的回调拿到的是首帧的 `t`，
之后切换语言，它弹出的消息仍是启动时的语言；`app://error` 与两条落点失败的回调因此用
`tLive()`（按当前存储的语言即时翻译），而不是 `t`。

反馈的时长收在 `FEEDBACK` 一处（`done` / `hint` / `notice` / `error` / `running` / `hold`），
`hold` 专用于进度会持续刷新、中途不能过期的长任务；`src/lib/utils.ts` 的 `messageFrom()` 是
把 unknown 变成文案的唯一途径。不可用控件统一用 `--dimmed` 与 `cursor: default`，
`:hover` 一律带 `:not(:disabled)`。

## 更新、分享与读取边界

2026-11 的全面评审补了一组不应被简化掉的约束。它们共同处理的是同一类问题：
**一个被声明为“已校验”的对象，在真正使用前仍可能变化，或声明值本身可能不可信。**

### 更新链路

| 约束 | 原因 |
| --- | --- |
| 清单里的文件名必须直接位于发布目录 | 路径分隔符、`..`、空格和 shell 元字符不能进入下载路径或交接参数；发布脚本产出的文件名只含 ASCII 字母、数字、`.`、`-`、`_` |
| 清单下载后限制 256 KiB | 清单是小型 JSON；损坏或被替换的 feed 不应无限占用内存 |
| 下载后校验是普通文件、精确字节数和 SHA256 | 只比较哈希仍会接受声明尺寸与实际尺寸不一致的输入 |
| 安装前再次执行同一组校验 | 下载文件位于可写的临时目录，下载完成到用户点击安装之间存在 TOCTOU 窗口；失败时必须删除文件 |
| macOS/Windows 辅助脚本使用固定脚本常量，路径作为进程参数传入 | 文件夹名不能改变 helper 的脚本文本；Windows `cmd.exe` 的参数解析仍需真机覆盖极端路径字符 |
| `Discard` 删除实际下载物 | 只清状态会留下可被误用的旧安装包 |

回归测试：`artifact_names_stay_inside_the_release_directory`、
`an_installer_changed_after_download_is_refused`，以及两个 handoff 脚本的参数化
检查。真实 Windows 交接仍需在 Windows 机器上验收。

### 分享链路

| 约束 | 原因 |
| --- | --- |
| peer 表最多 256 项，满时先清理过期项 | 未认证信标可伪造无限 id；可见列表不能无限增长 |
| 发送端按 header 声明大小精确复制 | 源文件在 metadata 之后变长时，多出的字节会被接收端误当成 ack |
| 源文件比声明短时立即失败 | 不允许用半截文件换取成功状态；接收端删除 `.shift-part` |
| 只有用户已接受后的接收失败才弹错误 | 任意 TCP 扫描或坏握手不应产生用户可见的 toast |
| 接收错误事件带文件名与 peer 名 | 失败要可定位，而不是只有一句泛化提示 |

回归测试：`the_peer_table_stops_accepting_new_ids_at_its_limit`、
`bytes_after_the_declared_size_never_cross_the_wire`、
`a_source_that_shrinks_is_refused_without_a_partial_file`。

分享协议本身仍无认证、无加密；这里修的是拒绝自动落盘、路径安全、字节边界和可见
失败，不是身份认证。安全边界见 `docs/SECURITY.md`。

### 编辑器读取

| 数据 | 上限 | 测试 |
| --- | --- | --- |
| 文本预览 | 8 MiB | `editor_reads_are_bounded_before_they_allocate` |
| 图片文件 | 64 MiB | 同上 |
| 图片像素 | 4000 万 | 与钉图上限一致 |
| 编辑后 data URL | 128 MiB | 解码前检查 |

文本与图片在 Rust 读盘前检查路径是否为普通文件并执行上限；UI 只负责把返回的
`errNotAFile` / `errTextTooLarge` / `errImageTooLarge` / `errEditDataTooLarge`
翻译成文案。不要在只读预览里静默吞掉读取错误。

## 修改这些路径时的统一回归命令

```bash
git diff --check

cd src-tauri
cargo fmt --check
cargo test --lib
cargo clippy --all-targets -- -D warnings

cd ..
pnpm check
pnpm test
pnpm build
```

发布构建还需额外完成本文对应章节的手工矩阵。任何一次修改如果改变了发现协议、转换
依赖链或 macOS 签名方式，都应在本文件补一条带日期的结论和提交号。
