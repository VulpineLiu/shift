# Shift 工程回归台账

本文件记录 0.1.2 到 0.1.7 之间已经踩过、并且不应再次引入的工程问题，重点覆盖：

- Drop / 局域网设备发现
- DOCX、Markdown 与图片转换
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
| 含文字和图片 DOCX → PDF | 先用 Pandoc `docx → html5`，通过 `--extract-media` 抽取图片到临时目录，再交给 `native/macos/doc-render.swift` | AppKit 的 OOXML 读取器会静默丢弃内嵌图片，所以这类 DOCX 不能直接交给它。 |
| Markdown → PDF | 先由 Pandoc（可用时）或内置 `markdown::to_html` 转成 HTML，再交给 AppKit 渲染器；HTML 的 base URL 指向 Markdown 源目录 | 相对图片路径依赖源目录，不能用临时 HTML 所在目录。 |
| Markdown → DOCX / RTF | Pandoc，并传 `--resource-path <源目录>` | Pandoc 的工作目录不是文档目录；缺少该参数会把相对图片替换成 alt 文本。 |
| Markdown → HTML | Pandoc，并传 `--resource-path <源目录>` 和 `--self-contained` | 输出必须是可单独移动的单文件，不能引用从未生成的 `media/` 目录。 |
| 含图 DOCX → HTML | Pandoc 加 `--self-contained` | 图片以内嵌 data URI 写入 HTML。 |
| 任意含宽图的文档 → PDF | `doc-render.swift` 的 `fitAttachments` 把超宽附件按正文栏宽度等比缩放 | 不重采样像素，只修正排版请求的尺寸，避免截图被裁成碎片。 |

### 不能随意改掉的约束

1. **不要把含图 DOCX 直接交给 AppKit OOXML 读取器。** 它会返回只有文字、没有附件的
   attributed string；问题发生在解析阶段，调页边距或打印参数无法找回图片。
2. **纯图片 DOCX 的页面抽取要排在通用渲染之前。** 它决定扫描件的真实页数和页面顺序。
3. **DOCX→HTML 后必须把 `--extract-media` 的临时目录作为 HTML base directory。**
   Pandoc 会把图片写成相对路径；base 指向源 DOCX 目录会找不到已抽取媒体。
4. **Markdown→PDF 的 HTML base URL 始终是源 Markdown 目录。** 即使 HTML 写在临时目录，
   也不能改用临时目录。
5. **Pandoc 转 DOCX/RTF/HTML 必须保留 `--resource-path`。** 这是相对图片丢失问题的
   根因修复，不应为了简化命令删掉。
6. **HTML 输出必须保留 `--self-contained`。** 否则用户拿到的是一个引用不存在媒体目录
   的 HTML 文件。
7. **宽图适配在 `doc-render.swift` 中完成。** 仅修改 CSS 宽度不会影响 AppKit 对这种
   HTML `<img>` 单元格的实际 attachment bounds。
8. **无 Pandoc 时保留 macOS 的降级路径。** 带图 DOCX 会退回旧的文本渲染，而不是让
   整个转换直接失败；修改时要明确产品选择，不能无意打破降级。
9. **Swift 渲染器改动后重新运行 `pnpm mac:drag-bridge`。** 只改源码不重编伴生进程，
   打出的 DMG 不会包含修复。

### 回归测试

自动测试集中在 `src-tauri/src/files.rs`：

- `renders_image_only_docx_pages_to_pdf`
- `keeps_pictures_when_rendering_a_text_docx_to_pdf`
- `fits_a_wide_picture_onto_the_page`
- `image_bearing_docx_to_html_embeds_images`
- `keeps_markdown_images_when_converting_to_word`

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
Shift-macOS-universal.dmg
```

它是 Apple silicon 与 Intel 通用的应用包，同时用于首次安装和软件内升级。更新清单的
`macos` 键必须指向这个 DMG，因为软件内升级脚本会用 `hdiutil` 挂载它再取出应用。
不再发布 `.pkg`；`scripts/macos-pkg.sh` 可以保留作历史工具，但不能进入正式发布目录或
更新清单。

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
