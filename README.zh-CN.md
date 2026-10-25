# Shift

**面向 macOS 与 Windows 的环形文件工作台。拖住文件，松开在某个扇区上，转换就在你自己的机器上完成。**

**语言：** [English](README.md) · **简体中文**

Shift 把文件格式转换、截图、录屏、标注、中英双语 OCR 全部收进一个环形菜单，它出现在你放下文件的地方。不上传、不注册、不经云端中转——每一个字节都在本地处理。

基于 Tauri 2、Rust、React 与 TypeScript 构建。原生截屏与拖拽行为封装在各平台适配器之后，因此环形菜单、编辑器与格式目录在所有平台上完全一致。

---

- [能力概览](#能力概览)
- [快速上手](#快速上手)
- [支持的文件](#支持的文件)
- [格式转换](#格式转换)
- [工具](#工具)
- [压缩、命名与取消](#压缩命名与取消)
- [截图与标注](#截图与标注)
- [OCR](#ocr)
- [发送到设备](#发送到设备)
- [快捷键](#快捷键)
- [菜单栏菜单](#菜单栏菜单)
- [输出位置](#输出位置)
- [外部依赖](#外部依赖)
- [平台支持情况](#平台支持情况)
- [从源码构建](#从源码构建)
- [架构](#架构)
- [仓库结构](#仓库结构)
- [隐私](#隐私)
- [已知限制](#已知限制)
- [常见问题](#常见问题)
- [参与贡献](#参与贡献)
- [许可证](#许可证)

---

## 能力概览

**环形菜单就是全部交互。** 在访达中按住 `Shift` 拖动文件（Windows 为 `Shift + Alt`），圆环会出现在光标下方。松开在某个格式上即完成转换；再加上 `Option`/`Alt` 可切换到工具环。主窗口同样接受拖放，其「文件转换」与「文件工具」按钮会先让你挑选文件，再弹出同一个圆环。

**一切按类型驱动。** 圆环只列出所选文件真正能变成的格式，并且不会重复显示文件自身的格式——图片可在 PNG/JPG/WEBP/HEIC/TIFF/BMP/GIF/PDF 之间转换，但当前格式不会出现目标；PPTX 给出 PDF 与 Markdown，而 `.key`、`.pages` 文件不会给出任何转换扇区，而不是等你点击后才失败。中心是毛玻璃预览，显示尺寸与体积；多选文件会渲染成一叠层叠图标。

**默认批量处理。** 选中十个文件、一次拖拽，就产出十个结果。结果保存在源文件旁边并附带描述性后缀（`report (compressed).pdf`、`photo (rotated).png`），永不覆盖——重名时自动追加序号。也可在设置中改为统一收集到一个文件夹。

**截图能力内置。** 冻结画面的区域截图、全屏截图，以及带悬浮计时器的区域录屏；一律对准光标所在的显示器，而非固定主屏。截图完成后直接进入标注画布，并可钉成常驻置顶的参考窗口。

**OCR 在本机运行。** 英文与简体中文，使用随应用打包的 Tesseract WASM 引擎与语言模型。OCR 全流程不发生任何网络请求。

**文件可以送到另一台电脑。** 每个运行中的 Shift 都会在局域网里广播自己的存在，因此「最近使用」列表可以像 AirDrop 那样把文件交给另一台电脑——点一下设备名，看着进度条走完，文件就出现在对方电脑的 Shift 输出文件夹里。

## 快速上手

Shift 常驻菜单栏（macOS）与通知区域（Windows），同时也有普通窗口。无需一直开着某个窗口——圆环在任何地方都能唤起。

**转换文件（Shift）**

1. 在访达或资源管理器中找到文件。
2. 按住鼠标左键把文件拖起来（不要松开）。
3. 拖动过程中按住 `Shift`，圆环会出现在光标周围。
4. 朝想要的格式方向拖动——该扇区会高亮放大，中心显示它的名称——然后松开鼠标。只需朝那个方向即可，不必精确对准。
5. 状态提示会显示「已保存 N 个文件」，结果就在原文件旁边。

> 💡 `Shift` 可以在开始拖动之后再按。圆环出现后就可以松开 `Shift`，它会一直保持到你松开鼠标。
> 💡 改变主意了？把鼠标移回中心（内圈 92 px 之内）再松开，什么都不会发生。
> 💡 靠近屏幕边缘时圆环会自动移回屏幕内；整个 760 px 的圆环会被约束在光标所在显示器范围内。

**使用工具（`Option + Shift`，Windows 为 `Alt + Shift`）** 操作相同，扇区会从格式换成该类型文件可用的工具。圆环打开时，保持按住 `Shift`，再按下或松开 `Option`/`Alt`，即可在格式环与工具环之间来回切换。

**不想拖动？** 主窗口中的「文件转换」与「文件工具」会先让你挑选文件，然后对选中的文件弹出同一个圆环；「文字识别」则选取图片并识别。直接把文件拖到主窗口上，会直接打开操作环。**拖到按钮上**则按钮会高亮并只执行它自己的操作：「文件转换」打开格式环，「文件工具」打开工具环，「文字识别」直接识别图片。

**一次处理多个文件。** 用 `⌘`/`Ctrl` 点选多个文件后一起拖动，每个文件都会被处理。选中 2 张以上图片或 2 个以上 PDF 时，会出现「合并」扇区，把它们合成一个 PDF。

**macOS 首次运行。** 需要授予「屏幕录制」（截图与录屏），以及你所选输出目录的「文件与文件夹」访问权限。截图前会先检查权限，因此被拒绝时给出的是可操作的提示，而不是一张全黑截图。

## 支持的文件

按扩展名识别，对应 `src-tauri/src/files.rs` 中的四张表。`kind` 决定中心图标以及会出现哪些扇区。**加粗**的扩展名是可以真正转换的；其余的只是能被识别，仅提供打开、在文件夹中显示，或提示你先去导出。

| 类型 | 扩展名 |
| --- | --- |
| `image` | **`png` `jpg` `jpeg` `webp` `tiff` `tif` `bmp` `gif` `heic` `heif` `avif`** |
| `video` | **`mp4` `mov` `m4v` `avi` `mkv` `webm`** |
| `audio` | **`mp3` `wav` `m4a` `aiff` `aif` `caf` `flac` `ogg`** |
| `pdf` | **`pdf`** |
| `document` | **`docx` `rtf` `odt` `txt` `md` `html` `htm`** `doc` `pages` |
| `presentation` | **`pptx`** `ppt` `key` |
| `spreadsheet` | **`xlsx` `xls` `xlsb` `ods` `csv` `tsv`** `numbers` |
| `other` | 未识别的类型——仅提供打开与在文件夹中显示 |

解码与编码能力并不对称，应用也不会假装对称：

- **图片解码**由 Rust `image` 原生完成，支持 PNG、JPEG、WebP、TIFF、BMP 与 GIF。HEIC/HEIF/AVIF 先走系统解码器——macOS 用 `sips`，其他系统用 ImageMagick——随后可转换为圆环提供的任意图片格式（包括 WEBP），也可生成 PDF。像素类工具仍需要可原生读取的输入，因此旋转、翻转、黑白、50%、压缩前请先转一次。
- **图片编码**中的 PNG、JPG、WebP、TIFF、BMP、GIF 为原生实现。HEIC 导出在 macOS 走 `sips`，在 Windows/Linux 需要 ImageMagick。AVIF 可作为输入，但暂未提供为导出目标。
- **音视频**的解码与编码范围取决于你的 FFmpeg 构建；菜单为视频提供 MP4、MOV、GIF、M4A，为音频提供 M4A、WAV、MP3。
- **Office 文件**以纯 Rust 解析（表格用 `calamine`，PPTX 用 `zip` + `quick-xml`），因此无需安装 Office。
- **可识别但不转换：** `.doc`、`.pages`、`.ppt`、`.key`、`.numbers` 会显示各自类型的图标，并提供编辑、打开、在文件夹中显示，但没有格式扇区。请先导出为 DOCX、PPTX 或 XLSX。

## 格式转换

以下是各文件类型在格式环中的选项。每一条转换路径都实现在 `src-tauri/src/files.rs`；文件自身的格式会在运行时自动排除。

| 源类型 | 圆环目标（不含源格式） | 引擎 | 说明 |
| --- | --- | --- | --- |
| 图片 | `PNG` `JPG` `WEBP` `TIFF` `BMP` `GIF` | Rust `image` | 编码器允许时走无损路径 |
| 图片 | `HEIC` | `sips` / ImageMagick | macOS 原生；其他系统需要 `magick` |
| 图片 | `PDF` | `printpdf` | 每张图一页，按 300 dpi 定尺寸 |
| 视频 | `MP4` `MOV` | FFmpeg | 优先 `libx264`，依次回退 `h264_videotoolbox`、`mpeg4` |
| 视频 | `GIF` | FFmpeg | 仅取前 15 秒、12 fps、宽 480 px、8 MB——设有上限，避免长时间录屏产出上百 MB 的 GIF |
| 视频 | `M4A` | FFmpeg | 仅保留音频轨，192k AAC |
| 音频 | `M4A` `WAV` `MP3` | FFmpeg | 若 FFmpeg 缺少 `libmp3lame`，MP3 回退到 `lame` |
| PDF | `TXT` `MD` | `lopdf` | 抽取文字层；扫描件会明确报错并引导使用文字识别 |
| PDF | `DOCX` | 页面渲染 + 原生 DOCX 封装 | macOS 用 PDFKit，Windows/Linux 用 Poppler；保留图片、图表和页面版式，不依赖 Microsoft Word |
| PDF | `RTF` | `lopdf` + Pandoc | 以文字层作为正文；需要 Pandoc |
| PDF | `HEIC` | PDFKit / Poppler + `sips` | 先渲染为 PNG 再转码；需要系统 HEIC 编码器 |
| PDF | `PNG` `JPG` `TIFF` | PDFKit / Poppler | 每页一个文件，输出到 `… pages` 文件夹 |
| 文档 `docx` `rtf` `odt` `txt` `md` `html` | `PDF` | AppKit 渲染器（macOS）· 其他系统 Pandoc | macOS 路径不需要 LaTeX 或 wkhtmltopdf |
| 文档 `docx` `rtf` `odt` `txt` `md` `html` | `DOCX` `RTF` `HTML` `TXT` `MD` | Pandoc | 自动省略源格式；Markdown 以 GFM 写出。HTML 为图片内嵌的独立单文件 |
| 演示文稿 `.pptx` | `MD` `PDF` | 纯 Rust + Pandoc | 按页生成标题、要点文字，**表格会输出为真正的 Markdown 表格**；PDF 仍是文字摘要版，非版面还原 |
| 电子表格 | `MD` `PDF` | `calamine` / 纯 Rust | 每个工作表一张 Markdown 表；`csv`/`tsv` 直接解析 |

表中没有的转换不会出现在界面上——圆环在运行时按这份目录生成，因此看得见的扇区就是能用的扇区。若缺少依赖，扇区会直接告诉你需要安装什么，而不是静默失败。

**页码进度遵循统一规则。** 能真实逐页处理的转换统一显示「正在转换第 N/M 页…」；当前包括图片型 DOCX→PDF 与带图 PDF→DOCX。一次性排版的文本 DOCX→PDF、以及纯文字 PDF→DOCX 无法可靠获得页数，因此显示「处理中…」，不会伪造页码。

> 说明：转换环不会重复列出文件自身的格式。例如 DOCX 不会出现 DOCX，避免无意义的整篇重写和版式损失。
> 扫描件/纯图片 PDF 没有文字层，选 `TXT`/`MD` 会返回明确报错——请改用「文字识别」，它会先把每页渲染成图片。

## 工具

工具环（拖拽时按 `Option + Shift`）。这些是一次手势完成的操作，不属于格式转换。

| 工具 | 适用对象 | 输出 | 具体行为 |
| --- | --- | --- | --- |
| 压缩 | 图片、视频、音频、PDF | `.jpg`/`.png`、`.mp4`、`.m4a`、`.pdf` | 图片按 q68 重编码（含透明通道则保留 PNG）；视频 CRF 28；音频转 128k AAC；PDF 先把内嵌图片降采样到 ≤1440 px 的 q60 JPEG，再清理对象 |
| 元数据 | 图片、PDF | 同类型，` (clean)` | 重写图片并去除 EXIF；移除 PDF 的 `/Info` 与 `/Metadata` |
| 旋转 | 图片、PDF | 同类型，` (rotated)` | 顺时针 90°，按各格式的原生方式处理 |
| 翻转 | 图片 | 同类型，` (flipped)` | 水平镜像 |
| 黑白 | 图片 | 同类型，` (bw)` | 转灰度 |
| 50% | 图片 | 同类型，` (50%)` | 宽高各减半，Lanczos3 |
| 裁剪 | 图片 | `.png`，` (cropped)` | 环形菜单中的裁剪会打开编辑器进行交互式框选 |
| 抠图 | 图片 | — | 打开编辑器，按采样到的背景色抠除背景 |
| 合并 | 2 张以上图片 | `.pdf`，` (images)` | 每图一页，按选中顺序排列 |
| 合并 | 2 个以上 PDF | `.pdf`，` (merged)` | 按选中顺序拼接 |
| 拆分 | PDF | `Page 001.pdf` … | 每页一个 PDF，输出到 `… pages` 文件夹 |
| 提取文字 | PDF | `.txt` | 仅文字层；扫描件请用「文字识别」 |
| 720p | 视频 | `.mp4`，` (720p)` | 缩放到 720p 高度，保持宽高比 |
| 静音 | 视频 | 同类型，` (muted)` | 去掉音轨，视频流直接复制、不重编码 |
| 截帧 | 视频 | `.png`，` (frame)` | 取首帧导出为静帧 |
| 提取音频 | 视频 | `.m4a` | 192k AAC |
| 单声道 | 音频 | `.m4a`，` (mono)` | 下混为单声道 128k AAC |
| 纯文本 | 文档 | `.txt` | 通过 Pandoc 导出纯文本 |
| 文字识别 | 图片、PDF | `.txt`，` (OCR)` | 见 [OCR](#ocr) |
| 编辑 / 标注 / 打码 / 加背景 | 图片 | — | 打开编辑器并预选对应工具 |
| 打开 · 在文件夹中显示 | 任意文件 | — | 交由系统默认应用或文件管理器 |

## 压缩、命名与取消

**「压缩」具体做了什么。**

| 输入 | 行为 |
| --- | --- |
| 图片 | 按画质 68 重新编码。含透明通道的图片写出 PNG，其余写出 JPG |
| PDF | 超过 1440 px 的内嵌图片先降采样为 q60 JPEG，再清理、重编号对象并压缩保存。若结果没有更小则不写出文件 |
| 视频 | 以 CRF 28 / 目标码率 2M 重编码，AAC 96k 音频，输出 MP4 |
| 音频 | 转为 128k AAC 的 M4A |

**输出命名——永不覆盖任何文件。** 结果保存在源文件旁边并附带描述性后缀；重名时追加 ` 2`、` 3`……而不是替换原文件。

```text
report (compressed).pdf      photo (rotated).png     slide (bw).png
report (clean).pdf           photo (50%).png         report (merged).pdf
```

PDF 转图片与 PDF 拆分会新建 `名称 pages` 文件夹，内含 `Page 001.png`、`Page 002.png`……再执行一次会生成 `名称 pages 2`，同样不会覆盖。

**取消手势。** 在圆环中心附近、或圆环范围之外松开鼠标，不会执行任何操作。

## 截图与标注

**截图。** 区域截图会先铺一层冻结画面，你框选的像素就是最终得到的像素；全屏截图则完全跳过遮罩层。录屏在 macOS 通过 `screencapture -v` 保存为 MOV，其他系统通过 FFmpeg 保存为 MP4，配有悬浮计时器与停止按钮。Linux 截图优先使用 `grim`，回退到 `gnome-screenshot`。

**标注。** 编辑器画布按原始分辨率工作，并写出新的 PNG，因此源文件永远不会被修改。

| 工具 | 行为 |
| --- | --- |
| 矩形、箭头、高亮、文字 | 自由绘制，每条标注可单独设置颜色与粗细 |
| 打码 | 实心黑色遮挡条，直接烧入输出图像——不是覆盖层 |
| 马赛克 | 对下方区域做真实采样降采样，不是半透明填充 |
| 序号 | 自动递增的序号标记 |
| 裁剪 | 交互式裁剪，带明确的「应用裁剪」步骤 |
| 加背景 | 圆角留白加渐变底，适合社交配图与幻灯片插图 |
| 抠图 | 采样四角、求平均背景色，并将其抠为透明 |
| 撤销 | 先回退标注，再回退图像级操作 |
| 保存副本 / 复制 | 另存为带后缀的新文件，或直接写入剪贴板 |
| 钉在屏幕上 | 将图片浮为置顶窗口，带复制与关闭按钮 |

**编辑器。** 图片打开标注画布；视频打开播放器；`txt` `md` `csv` `tsv` `html` `htm` 以可编辑文本打开。每个编辑器都提供复制、另存为、打开、在文件夹中显示、文字识别与复制文字。

## OCR

界面中对应「文字识别」（图片与扫描件）和「提取文字」（含文字层的 PDF）两处按钮。

使用 Tesseract.js 的 LSTM 内核，以 WASM 形式在应用内运行。`eng` 与 `chi_sim` 语言模型在构建时由 `pnpm prepare:ocr` 复制到 `public/tesseract`，因此识别无需联网，首次运行也不会下载任何东西。

- **图片**——单次识别，实时显示进度，结果可进入编辑器或剪贴板。
- **扫描版 PDF**——先把各页渲染成图片，再逐页识别并显示每页进度，结果写入 ` (OCR)` 文本文件，页与页之间用 `--- Page N ---` 分隔。
- **含文字层的 PDF**——请改用「提取文字」，它直接读取内嵌文字层，速度远快于 OCR。

## 发送到设备

Shift 会持续维护局域网内其他 Shift 实例的列表，并据此提供 AirDrop 式传输。把鼠标移到「最近使用」的任意一行点击发送按钮，或打开文件的「文件工具」环并松开在「发送」扇区上——两种入口都会弹出每台设备一张卡片的设备选择面板，点击卡片即刻开始传输。工具环对每一种文件类型都提供「发送」，选中多个文件时按一批一起发送。

没有对方同意，文件不会落地：接收方会弹出一张小卡片，写明发送者与文件名，并提供「接收」「拒绝」两个按钮以及倒计时。点击拒绝、关掉卡片或任其超时，都会在文件字节开始传输之前就中止本次分享。多文件批次只会询问一次，同一批的其余文件随后自动接收。接收后的文件写入对方 Shift 的输出文件夹，出现在对方的「最近使用」列表里，同时弹出提示告知是谁发来的。同名文件自动追加 ` 2`、` 3`，传输中断不会留下写了一半的文件。

无需服务器、无需配置，发现机制如下：

- 每个实例每两秒向组播组 `239.255.77.88:47888` 发送一次小型 UDP 广播，并额外发送一份传统广播，以兼容丢弃组播的网络。广播内容包含机器名、操作系统，以及用于接收文件的动态 TCP 端口。
- 九秒内没有再次收到广播，设备就会从列表移除，因此切换网络后过期条目会自动消失。
- 文件字节通过直连 TCP 传输：`SHSF` 握手、JSON 头、文件内容，最后一字节确认；只有接收方已完整落盘，发送方才会报告成功。

整个过程只涉及本地网络——不会有任何数据走到互联网。接收端监听全部网卡，因此 macOS 首次分享时会弹出防火墙授权提示，允许后双向均可用。

## 快捷键

| 操作 | macOS | Windows / Linux |
| --- | --- | --- |
| 区域截图 | `⌥⇧2` | `Alt+Shift+2` |
| 区域录屏 | `⌥⇧3` | `Alt+Shift+3` |
| 全屏截图 | `⌥⇧4` | `Alt+Shift+4` |
| 拖拽时显示格式环 | 按住 `⇧` | 按住 `Shift` |
| 拖拽时显示工具环 | `⌥⇧` | `Alt+Shift` |

三个截图快捷键注册为全局快捷键。拖拽圆环由修饰键驱动：在开始拖动之前就按住修饰键，圆环会直接出现在光标处，无需先让 Shift 获得焦点。

## 菜单栏菜单

菜单栏（macOS）与通知区域（Windows）图标提供这些快捷入口：

| 菜单项 | 快捷键 | 作用 |
| --- | --- | --- |
| Open Shift | — | 显示主窗口 |
| Capture area | `Alt+Shift+2` | 开始区域截图 |
| Capture full screen | `Alt+Shift+4` | 截取光标所在显示器 |
| Record area | `Alt+Shift+3` | 开始或停止区域录屏 |
| OCR an image… | — | 选择图片并在本机识别文字 |
| Quit Shift | `Cmd/Ctrl+Q` | 退出；若有正在进行的录屏会先收尾保存 |

菜单项文案会跟随应用语言；macOS 系统权限弹窗仍为英文。

设置面板的「行为」区有两个开关：**拖拽圆环**（Shift 拖拽手势的总开关）与**开机时启动**。

## 输出位置

| 结果 | 默认位置 |
| --- | --- |
| 截图 | `~/Pictures/Shift` |
| 录屏 | `~/Movies/Shift` |
| 文件操作 | 源文件所在文件夹 |

三者都可在设置中修改。文件操作另外提供「统一收集到一个文件夹」模式，替代默认的就近保存。设置面板会列出所有解析后的实际路径、检测该路径是否可写、在文件管理器中显示它，并在缺少权限时直接跳转到 macOS 的「屏幕录制」与「文件与文件夹」隐私面板。

## 外部依赖

核心转换、环形菜单、截图与 OCR 在零外部依赖的情况下即可工作。以下工具用于扩展能力边界：

| 工具 | 用于 | macOS | Windows | Linux |
| --- | --- | --- | --- | --- |
| FFmpeg | 音视频转换/压缩、720p、静音、截帧、单声道；非 macOS 的录屏 | 可选 | 录屏**必需** | **必需** |
| Pandoc | 文档 ↔ DOCX/RTF/HTML/TXT/MD（仅支持 ODT 输入） | 可选 | 需要 | 需要 |
| Typst | Pandoc 文档转 PDF 引擎 | 不需要（AppKit） | 需要 | 需要 |
| Poppler `pdftoppm` | PDF 转图片、PDF 转 Word、扫描件 OCR | 不需要（PDFKit） | 需要 | 需要 |
| ImageMagick `magick` | HEIC/HEIF/AVIF 输入与 HEIC 输出 | 不需要（`sips`） | 需要 | 需要 |
| LAME | 当 FFmpeg 缺少 `libmp3lame` 时的 MP3 编码 | 少见 | 少见 | 少见 |

Shift 会在设置 →「运行组件」中显示当前机器实际找到的工具、版本和路径，并可一键安装缺失的 Pandoc、Typst、FFmpeg、Poppler 与 ImageMagick。macOS 使用 Homebrew，Windows 使用 WinGet；安装后在界面中自动重新检测，无需重启。工具在调用时也会从 `PATH` 及系统常见安装目录查找，因此缺失某个依赖只会影响对应的扇区，不会影响整个应用。

## 平台支持情况

| 能力 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| 环形菜单、编辑器、托盘、多语言 | 支持 | 支持 | 支持 |
| 拖拽唤起圆环 | AppKit 全局拖拽桥 | Win32 拖拽状态轮询 + 原生拖放 | 暂不支持——仅能用主窗口选择文件 |
| 区域 / 全屏截图 | `screencapture` | PowerShell | `grim` / `gnome-screenshot` |
| 跟随光标所在显示器 | 支持 | 支持 | `grim` 输出选择 |
| 区域录屏 | `screencapture -v` | FFmpeg | 计划中 |
| 图片转换与编辑 | 支持 | 支持 | 支持 |
| HEIC / AVIF 编码 | `sips` | ImageMagick | ImageMagick |
| PDF 文字、合并、拆分、旋转、压缩 | 支持 | 支持 | 支持 |
| PDF 转图片 | PDFKit 伴生进程（HEIC 需 `sips`） | Poppler（HEIC 需 ImageMagick） | Poppler（HEIC 需 ImageMagick） |
| PDF 转 Word | 内置页面渲染器 | 需 Poppler | 需 Poppler |
| 扫描件文字识别 | 支持 | 需 Poppler | 需 Poppler |
| 文档转 PDF | 内置 AppKit 渲染器 | Pandoc + PDF 引擎 | Pandoc + PDF 引擎 |
| PPTX / XLSX / ODS 转 Markdown | 支持（纯 Rust） | 支持 | 支持 |
| 本地中英文字识别 | 支持 | 支持 | 支持 |

## 从源码构建

需要 Node.js 20+、pnpm 8+、Rust 1.90+；构建 macOS 原生伴生进程还需 macOS 13+ 与 Xcode 命令行工具。

```bash
pnpm install
pnpm mac:drag-bridge   # 仅 macOS —— AppKit 拖拽桥、PDFKit 与文档渲染器
pnpm tauri dev
```

`pnpm mac:drag-bridge` 会把 `native/macos/*.swift` 编译为 `src-tauri/binaries` 下的分架构与通用（universal）伴生进程。Windows 与 Linux 完全跳过这一步。

```bash
pnpm check          # TypeScript 类型检查，不产出文件
pnpm test           # 转换矩阵与拖拽命中测试
pnpm build          # tsc + vite
cargo test --lib    # Rust 单元测试，在 src-tauri/ 目录下执行
pnpm tauri build    # 发布构建
```

macOS 通用（Universal）构建：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm mac:drag-bridge
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" \
  RUSTUP_TOOLCHAIN=stable \
  pnpm tauri build --target universal-apple-darwin --bundles app,dmg
```

产物是单个通用 Mach-O，内含通用版的拖拽、PDF 渲染与文档渲染伴生进程。若要在 macOS 上同时生成 Windows 安装版与便携版，运行 `scripts/build-windows-release.sh`；完整步骤见 `docs/BUILD_WINDOWS.md`。

**权限（macOS）。** 截图与录屏需要「屏幕录制」，输出目录需要「文件与文件夹」授权，带声音录屏需要「麦克风」。Shift 会在打开截图遮罩前先检查屏幕录制权限，因此被拒绝时给出的是可操作的提示，而不是一张全黑的截图。使用本地证书签名的开发构建，可以在反复重编译后保持这些授权稳定；对外分发则需要 Developer ID 证书与公证。

## 架构

```text
React UI                     单一 webview，六种视图
  ├─ main        拖放目标、快捷键、最近输出、语言、设置
  ├─ radial      透明置顶圆环，按极角扇区命中判定
  ├─ capture     冻结画面 + 区域框选
  ├─ recorder    悬浮计时器与停止按钮
  ├─ editor      标注画布、文本、视频、OCR
  └─ pin         置顶图片参考窗

Tauri 命令层                  src-tauri/src/
  ├─ files.rs      检视、转换、压缩、元数据、旋转、裁剪、音视频
  ├─ pdf.rs        合并、拆分、旋转、文字、渲染、图片转 PDF、压缩
  ├─ office.rs     PPTX 与电子表格转 Markdown，纯 Rust
  ├─ capture.rs    全屏截图、区域裁剪、交给编辑器
  ├─ recording.rs  screencapture / FFmpeg，以中断方式收尾
  ├─ drag_bridge.rs  全局拖拽检测、圆环模式、环形菜单定位
  ├─ settings.rs   输出目录、可写检测、隐私面板深链
  ├─ share.rs      局域网广播、设备列表、AirDrop 式文件传输
  └─ editor.rs     编辑器与钉图窗口状态

平台适配器
  ├─ macOS    AppKit 拖拽桥 · PDFKit 渲染器 · AppKit 文档渲染器 · screencapture
  ├─ Windows  Win32 拖拽状态 · PowerShell 截图 · FFmpeg · Poppler
  └─ Linux    grim / gnome-screenshot · FFmpeg · Poppler

OCR：Tesseract.js WASM + 内置 eng/chi_sim，进程内运行，完全离线
```

环形菜单的命中判定是纯几何计算——同一套极角运算同时服务于原生指针事件、webview 悬停与 Windows 的拖放数据，因此圆环行为不会在各平台之间产生偏差。

## 仓库结构

```text
src/                 React 界面、多语言（en/zh）、Tauri API 绑定
src/lib/             纯函数与 Node 测试，包括格式转换矩阵
src-tauri/src/       Rust 命令层与平台适配器
native/macos/        Swift 伴生进程：拖拽桥、PDF 渲染器、文档渲染器
                     由 pnpm mac:drag-bridge 编译进 src-tauri/binaries/
scripts/             OCR 资源准备、macOS 伴生进程、Windows 安装器与发布构建
public/tesseract/    WASM 内核与语言模型，构建生成，不提交
assets/              应用图标源文件
src-tauri/icons/     各平台生成的应用图标
docs/                Windows 构建指南、路线图
.github/workflows/   macOS 与 Windows CI
FileFlipper/         独立姊妹项目，拥有自己的仓库
```

## 隐私

Shift 不发起任何互联网请求。文件在本地读取、转换、写回；OCR 在进程内基于内置模型运行；最近输出只是本地列表。既不上传，也不收集。

唯一的例外取决于你是否使用：**发送到设备** 只与本地网络通信——`239.255.77.88:47888` 上的 UDP 广播，加上两台电脑之间的直连 TCP，其中携带机器名称以便双方互相标注。在你打开该面板、或有另一台 Shift 向你发送文件之前，它不会有任何动作。

## 已知限制

- Windows/Linux 上的文档转 PDF 仍需 Pandoc 加上可用的 PDF 引擎；内置的 AppKit 渲染器仅限 macOS。
- PPTX 与电子表格转 PDF 是文字与表格摘要，不是版面精确还原。
- `.doc`、`.pages`、`.ppt`、`.key`、`.numbers` 可识别但不解析——请先导出为 DOCX/PPTX/XLSX。
- HEIC/HEIF/AVIF 通过系统解码器转换，但像素类工具需要 PNG/JPEG/WebP/TIFF/BMP/GIF 输入；AVIF 暂未提供为导出目标。
- PDF 压缩会把内嵌图片降采样到最长边 1440 px、q60 JPEG。凡是不能确认可安全往返的内容都会原样保留——蒙版、索引色/CMYK、JPX/JBIG2/CCITT——因此部分特殊扫描件仍难以压缩。
- Windows 的录屏与音视频转换要求 `PATH` 中存在 `ffmpeg`。
- 局域网分享依赖允许点对点流量的网络。部分访客网络与企业 Wi-Fi 会丢弃组播；这些网络仍可经由广播与单播回退路径工作，但若路由器隔离了客户端，两个实例便无法相遇。
- Windows 上每台电脑只有一个 Shift 实例能占用发现端口，因此额外实例不会出现在设备列表中。
- Windows 代码路径可编译并通过 CI，但手势层面的验证仍需真实 Windows 设备。
- macOS 上签名身份变化后，可能需要重新授予「屏幕录制」与「辅助功能」权限。

路线图见 `docs/ROADMAP.md`。

## 常见问题

**❓ 按住 Shift 拖文件，圆环没出现。**
一定要先开始拖动——按住鼠标并把文件拖过拖动阈值——在拖动过程中按住 `Shift`。只选中文件再按 `Shift` 是没有反应的。Windows 需要在拖拽越过系统阈值的那一刻按住 `Shift`；同一时刻再按 `Alt` 则进入工具环。

**❓ 提示这类文件没有可用格式。**
未识别的扩展名仅提供打开与在文件夹中显示；旧版 Apple/Office 格式（`.doc`、`.pages`、`.ppt`、`.key`、`.numbers`）刻意不提供转换。请先导出为 DOCX、PPTX 或 XLSX。

**❓ 能开机自启吗？能一键停用吗？**
都可以，位置在设置面板的「行为」区。**拖拽圆环**是 Shift 拖拽手势的总开关：关闭后不会再出现任何圆环，而托盘、主窗口与快捷键照常可用。**开机时启动**会在 macOS 写入 LaunchAgent、Windows 写入 `HKCU\...\Run`、Linux 写入 autostart `.desktop`，同一开关即可撤销。

登录项只对当前用户生效，并指向 .app 包本身，因此 macOS 的屏幕录制授权身份在多次启动间保持一致。若移动了应用位置，请重新切换一次该开关。

**❓ 我的文件会被上传吗？**
不会。代码里不存在上传路径，转换与 OCR 流程也不发起任何网络请求；Tesseract 模型随应用一起打包。不收集数据，也没有账号体系。

**❓ 输出文件去哪儿了？**
默认在源文件旁边，带描述性后缀。可先看主窗口的最近列表，点击条目即可打开结果；设置面板会列出全部实际输出路径并可在文件管理器中显示。

**❓ 支持哪些语言？**
英文与简体中文，可在主窗口的设置中切换；首次启动时跟随系统语言。托盘菜单会跟随所选语言；macOS 系统权限弹窗仍为英文。

**❓ 能先标注、不保存原图吗？**
可以。标注工作在画布副本上，只有「保存副本」或「复制」才会写盘，源文件永不被修改。也可以「钉在屏幕上」，把它作为浮动参考图。

**❓ 怎么卸载？**
从托盘退出，然后删除应用即可。除你指定的输出目录和应用自身的配置/缓存目录外，不会写入其他位置。

## 参与贡献

新增一种格式 = 在 `src/lib/conversionFormats.ts` 的 `conversionFormats` 中加一项、补充对应矩阵测试，并在 `src-tauri/src/files.rs` 的 `perform_action` 中加对应处理；新增一个工具 = 在 `toolWedges` 加一项加一个 match 分支。推送前请运行 `pnpm check`、`pnpm test`、`pnpm build` 与 `cargo test --lib`。CI 会在 macOS 与 Windows 上产出 debug 包。

**欢迎二次开发。** 你可以免费使用、修改、再发布本代码，也可用于商业项目，只需保留原版权声明与 LICENSE 文件。

## 许可证

以 **MIT 许可证**发布——见 [LICENSE](LICENSE) 与 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

本应用构建使用了 Tauri（MIT / Apache-2.0）、React（MIT）、Lucide（ISC），以及 Tesseract.js 与其英文、简体中文模型（Apache-2.0）。
