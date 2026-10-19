# AppContainer 回环豁免工具 `enable-loopback`

**日期：** 2026-10-19

**状态：** 设计草案第二版（已纳入 2026-10-19 评审决定），剩余待决见 §9。实现未开始。

**基线：** 主仓 `main@d72ad2af0`；被替换的工具为 `backend/tauri/resources/enableLoopback.exe`（84 040 字节，.NET Framework 4 WinForms，来自 `Kuingsmile/uwp-tool` 的 `latest` release，无源码）；UI 框架 `windows-reactor`（microsoft/windows-rs，WinUI 3），crates.io 最新 0.100.0，master `a29e064`（2026-10-19）。

**需求：** 在 `https://github.com/libnyanpasu/enable-loopback.git` 新建 Rust 实现，用 `windows-reactor` 搭 WinUI 3 界面；以 `runas` 提权；支持 i18n；功能完整复刻原工具。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

**行为来源：** 以 `ilspycmd` 反编译原 exe 得到的源码为准（`frmMain.cs`、`Program.cs`、`IsolationAPI/*.cs`），下文 §3 每条都可在反编译源码里找到对应。

## 1. 决策与范围

1. **仓库与布局（已定）**：独立仓库 `libnyanpasu/enable-loopback`，以 git submodule 挂在主仓 `backend/enable-loopback`（与 `backend/nyanpasu-runtime` 同级）。子仓是自己的 cargo workspace，唯一成员 `crates/enable-loopback`；主仓 `backend/Cargo.toml` 把它加入 `exclude`（与 `nyanpasu-runtime` 同理）。产物落地文件名保持 `enableLoopback.exe`，主仓调用方只需追加 `--lang`（见 §8）。
2. **crate 内分层**：
   - `netiso`：FirewallAPI 封装，纯数据进出，不依赖 UI；
   - `model`：列表状态、脏标记、排序、复制格式化、命令行过滤规则等纯逻辑，可用普通值测试；
   - `ui`：`windows-reactor` 组件；
   - `cli`：命令行模式；
   - `i18n`：文案。
3. **提权**：沿用主仓现有方式——`backend/tauri/src/core/win_uwp.rs` 用 `runas` 启动，进程本身已提权。exe 清单保留原工具的 `requestedExecutionLevel level="highestAvailable"`，让用户直接双击运行时也能得到管理员令牌。不做自提权、不做 helper 进程。
4. **不做的**：新增功能（搜索、筛选、批量清理孤儿等；唯一例外是工具栏的豁免计数，见 §4.2）、Windows 8 版本检查（WinAppSDK 2.x 最低就是 Windows 10 1809，检查已无意义）、x86 构建。只发布 x64 与 arm64。
5. **原工具的 Fiddler 品牌**（`Learn more` 指向 `api.getfiddler.com`、悬停显示 Progress 版权）要替换，见 §3.9。
6. **修正原工具的保存缺陷**（已定）：见 §3.5。除此以外的行为照原样复刻。

## 2. 系统接口

全部为用户态 API，**不需要内核驱动**。

| 用途 | API | 备注 |
| --- | --- | --- |
| 枚举 AppContainer | `NetworkIsolationEnumAppContainers(flags, &count, &list)` | 首次加载 `flags = 0`；手动刷新 `flags = NETISO_FLAG_FORCE_COMPUTE_BINARIES (1)`（与原工具一致） |
| 释放枚举结果 | `NetworkIsolationFreeAppContainers` | 用 RAII guard 保证释放 |
| 读取豁免名单 | `NetworkIsolationGetAppContainerConfig(&count, &sids)` | 返回 `SID_AND_ATTRIBUTES[]`，用 `LocalFree`/`HeapFree` 释放（需核对文档，原工具用 `FreeHGlobal`=`LocalFree`） |
| 写入豁免名单 | `NetworkIsolationSetAppContainerConfig(count, sids)` | **整表覆盖**；返回 5 (`ERROR_ACCESS_DENIED`) 表示未提权 |
| 解析资源字符串 | `SHLoadIndirectString` | 仅当字符串以 `@` 开头；失败则保留原文（截图里大量 `@{...}` 就是解析失败的原文） |
| SID → 账户名 | `LookupAccountSidW` | 结果为 `本机名\用户` 时去掉本机名前缀；失败则显示 SID 字符串 |
| SID ↔ 字符串 | `ConvertSidToStringSidW` / `ConvertStringSidToSidW` | 注册表回退模式用 |
| 注册表回退 | `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings` | 子键名为 AC SID，值 `DisplayName`、`Moniker` |

绑定来源：`windows` crate 的 `Win32_NetworkManagement_WindowsFirewall`、`Win32_Security`、`Win32_UI_Shell`、`Win32_System_Registry`。具体 feature 名实施时以 docs.rs 为准。

### 2.1 数据模型

```rust
struct AppContainer {
    sid: Sid,                     // 主键；按 SID 合并多用户
    users: Vec<String>,           // 排序后显示为 "a; b"
    name: Option<String>,         // appContainerName
    display_name: Option<String>, // 已解析
    description: Option<String>,  // 已解析
    package_full_name: Option<String>,
    binaries: Vec<String>,
    exempt_at_last_check: bool,
}
```

- 同一 SID 对每个用户各返回一条记录，按 SID 合并，`users` 追加后排序（原工具行为）。
- 缺失字段的占位文案（需 i18n）：`(No DisplayName)`、`(No Description)`、`(No Package Name)`、`(No AC Name)`、`(No AC SID)`、`(No User SID)`、Binaries 为空时 `(None)`。注意：原工具只对 `null` 用占位，空串照原样显示为空（截图第一行 DisplayName 为空即此情况），复刻此差别。
- 注册表回退模式下：`description`、`package_full_name` 为 `?`，`users` 为当前用户 `域\用户名`，`binaries` 为空。

## 3. 功能细节（复刻清单）

### 3.1 启动

| 条件 | 行为 |
| --- | --- |
| 首个参数（去掉前导 `/`、`-`，小写）为 `all`/`none`/`allow`/`deny` | 进入命令行模式（§3.8），不显示窗口 |
| 首个参数含 `?` | 弹出语法说明，退出码 `-10` |
| 已有实例在运行（命名互斥量 `EnableLoopbackTool`，等 250 ms） | 找到其窗口则置前并退出（`-3`）；找不到则提示"已在运行但无法切换"（`-5`） |
| 命令行含 `manual` | 打开窗口但不自动刷新 |
| 命令行含 `-registry` | 刷新时跳过 API，直接读注册表 |
| 其他 | 打开窗口并自动刷新（不计算 binaries） |

互斥量名沿用 `EnableLoopbackTool`，这样新旧工具互斥。置前方式见 §4.1。

### 3.2 刷新

1. 若有未保存修改：确认框"尚未保存修改，仍要刷新吗？"，默认按钮为"否"。
2. 清空列表，禁用全部按钮与列表，状态栏"正在刷新 AppContainer 信息…"，显示进度指示。
3. 后台线程（`spawn_background`）执行：
   - 非 `-registry`：调用枚举 API。失败时提示"无法用 NetIsoEnumAppContainers 获取列表，错误码 0x…，将改为读取注册表，部分信息不可用"，然后走注册表回退；
   - 回退仍失败：报错（见 §3.7）。
   - 读取豁免名单，标记 `exempt_at_last_check`。
4. 豁免名单里有、枚举结果里找不到的 SID 为**孤儿记录**，逐条弹窗（§4.4）。原工具在后台线程弹 MessageBox；新实现在后台收集孤儿列表，回到 UI 线程后依次弹 `ContentDialog`。
5. 完成：填充列表，勾选状态 = `exempt_at_last_check`；状态栏"已于 HH:mm:ss 刷新 AppContainer 信息。"（时间按当前语言的长时间格式）；列表非空时启用列表和"全部豁免/全部取消"；刷新按钮总是重新启用；焦点移到列表。

### 3.3 勾选与脏状态

- 勾选框即"是否豁免"。
- 任一行勾选状态 ≠ `exempt_at_last_check` 即为脏：启用"保存修改"，状态栏"点击『保存修改』以提交。"；否则禁用保存、清空状态栏。
- 刷新填充期间不触发脏检查（原工具会临时解绑事件）。

### 3.4 全部豁免 / 全部取消

- 按钮与右键菜单均可触发。
- 全部勾选后若为脏，状态栏"所有 AppContainer 待豁免。点击『保存修改』以提交。"；全部取消对应"所有 AppContainer 待移除豁免。…"。

### 3.5 保存

1. 先禁用保存按钮，防止重复提交。
2. 用所有**已勾选**行的 SID 调用 `SetAppContainerConfig`。
3. 成功：才把各行 `exempt_at_last_check` 更新为当前勾选状态；状态栏"回环豁免配置已更新。"
4. 返回 `ERROR_ACCESS_DENIED`：提示"必须以管理员身份运行才能修改"，状态栏"未保存。需要管理员权限。"
5. 其他错误：提示错误信息（含 `0x…`），状态栏"未保存。"
6. 失败（4、5）时不改 `exempt_at_last_check`，勾选保持用户的修改，列表仍为脏，保存按钮重新启用。

**与原工具的差异（已定修正）**：原工具在调用 API **之前**就改写 `exempt_at_last_check`，失败后列表被误判为干净、保存按钮一直禁用，用户的修改既没写入也无法重试。

**原样保留并在界面说明**：该 API 整表覆盖，保存时孤儿记录会被删除。存在孤儿记录时，保存前的状态栏追加提示"保存将移除 n 条孤立记录"。

### 3.6 列表交互

| 交互 | 行为 |
| --- | --- |
| 点击列头 | 按该列排序；再次点击同列切换升/降序；比较为**序数**比较（`Ordinal`）；排序后滚动到首个选中行 |
| 多选 | 原工具支持（`FullRowSelect` + 多选）；reactor 无选中集合回调，自建选中模型，见 §6.2 |
| `Ctrl+A`（焦点在列表） | 全选 |
| `Ctrl+C` / 右键"复制选中行" | 每行：`LoopbackExempt` 或 `LoopbackBlocked` + 7 列，用 `\t` 分隔；选中多于一行时每行以 `\r\n` 结尾，只有一行时不加换行 |
| 右键"复制选中列" | 取右键所在列，复制每个选中行该列的值，每行以换行结尾 |
| 右键菜单 | 复制选中行、复制选中列、分隔线、全部取消、全部豁免 |
| 当前用户的行高亮 | `users` 含当前 `域\用户名` 的行用高亮底色；高对比度模式下换用系统色 |
| 关闭窗口 | 有未保存修改时确认"尚未保存，仍要退出吗？"，默认"否" |

`复制选中行`的状态前缀 `LoopbackExempt`/`LoopbackBlocked` 是机器可读格式，**不翻译**。

### 3.7 错误提示

| 情况 | 状态栏 | 弹窗 |
| --- | --- | --- |
| 枚举错误码 1753 (`EPT_S_NOT_REGISTERED`) | "获取 AppContainer 信息失败 [0x6d9]" | 标题"Windows 防火墙已停用"，说明第三方安全软件可能停用或替换了防火墙服务 |
| 其他枚举错误 | 同上格式 | "无法获取本机的 AppContainer 列表" + 详情 |
| 其他异常 | "失败：{message}" | "操作失败" + 详情 |

### 3.8 命令行模式

| 参数 | 行为 |
| --- | --- |
| `-all` | 豁免所有 AppContainer |
| `-none` | 清空豁免名单（直接写空表，不枚举） |
| `-allow NAME` | 新名单 = 已豁免 ∪ AC 名为空 ∪ AC 名包含 NAME（忽略大小写） |
| `-deny NAME` | 新名单 = 已豁免 且（AC 名为空 或 AC 名不含 NAME）。`-deny *` 等同 `-none` |
| `-allow`/`-deny` 缺 NAME | 提示"必须指定部分名称，用 * 匹配全部"，退出码 `-1` |
| 无法枚举 | 写入调试输出（`OutputDebugString`），退出码 `-10` |
| 其他失败 | 调试输出，退出码 `-1` |
| 成功 | `0` |

原工具的解析是"在拼接后的整条命令行里找子串"，因此 `-allow` 会优先于 `-all` 命中。新实现改为按参数逐个解析，但对上述合法输入结果一致；`-allow` 中 NAME 允许包含空格（剩余参数用空格拼接）。

命令行模式不创建窗口，也不初始化 WinAppSDK——即使机器上没有 Windows App Runtime，命令行模式也能用。

### 3.9 帮助与版本

- `F1` 或 `Alt+L`：打开"了解更多"链接。
- 链接目标（已定）：Microsoft Learn《How to enable loopback and troubleshoot network isolation (Windows Runtime apps)》，`https://learn.microsoft.com/previous-versions/windows/apps/hh780593(v=win.10)`。URL 不带语言段，Learn 按浏览器语言自动跳转本地化版本（2026-10-19 验证 en-us、zh-cn 均返回 200）。该页属于"以前的版本"归档，但仍是微软对回环豁免与 `CheckNetIsolation` 的正式说明。
- 悬停链接时状态栏显示 `enable-loopback v{版本} {64bit|arm64} {系统版本}`，替代原 Progress 版权。

## 4. UI 设计

### 4.1 窗口

- 标题：本地化（已定）"AppContainer 回环豁免工具"。
- 图标（已定新绘）：按 Fluent Design 的 Windows 11 应用图标风格（无底板、几何隐喻、形状内柔和渐变、轻微层叠阴影）手绘 SVG，先由 codex 出概念图再定稿，源文件 `crates/enable-loopback/assets/icon.svg` 入库。另备一份 16px 简化版 SVG，保证小尺寸可辨识。构建时由 `build.rs` 用 `resvg` 栅格化为 16/20/24/32/48/256 多尺寸 `.ico`，通过资源脚本嵌入 exe（资源管理器与任务栏图标）。窗口图标不用 reactor 的 `icon(path)`（它调用 `AppWindow::SetIcon`，需要磁盘上的 `.ico` 路径），而是在 `run_window` 回调里用 `LoadImageW` 从自身资源加载并 `WM_SETICON`，免得运行时释放文件。具体图形见 §9-1。单实例置前不依赖标题：启动时把本实例的 HWND 写入命名共享内存（与互斥量同名前缀），第二个实例从中读取后置前。这样各语言之间也能互相置前；与旧工具（英文标题）并存时无法互相置前，可接受。
- 初始客户区 960×600 DIP，最小 640×400（原工具 761×352 / 516×270，按 WinUI 控件尺寸放大）。
- 背景 `Mica`，跟随系统亮/暗色；高对比度下使用系统资源。
- DPI 感知由 WinUI 负责。

### 4.2 布局

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ AppContainer 回环豁免工具                                    ─  □  ✕     │
├──────────────────────────────────────────────────────────────────────────┤
│ ⓘ 出于安全与可靠性考虑，Windows 禁止 UWP/"沉浸式"应用向本机发送网络流量。│  InfoBar(信息, 不可关闭)
│   本工具可为调试目的解除此限制。                         [了解更多]       │
├──────────────────────────────────────────────────────────────────────────┤
│ [⟳ 刷新] │ [☑ 全部豁免] [☐ 全部取消] │ [💾 保存修改]           12/248   │  CommandBar + 计数
├──┬──────────────┬──────────────┬──────────┬─────────┬────────┬──────┬────┤
│☐ │显示名称 ▲    │描述          │包        │AC 名称  │AC SID  │用户  │文件│  列头(可点击排序)
├──┼──────────────┼──────────────┼──────────┼─────────┼────────┼──────┼────┤
│☑ │1Password     │Protect and…  │Agilebit… │Agilebi… │S-1-15… │a6320 │(无)│  当前用户行高亮
│☐ │@{Microsoft.… │@{Microsoft.… │Microsoft…│Microso… │S-1-15… │NT AU…│(无)│
│  │  …                                                                    │  ListView(虚拟化)
├──┴───────────────────────────────────────────────────────────────────────┤
│ 已于 22:09:12 刷新 AppContainer 信息。                                    │  状态栏
└──────────────────────────────────────────────────────────────────────────┘
```

| 区域 | reactor 控件 | 说明 |
| --- | --- | --- |
| 说明 | 仿 Informational `InfoBar` 的 `Border`（CardBackground/CardStroke、圆角 4）内一行 `Grid`：状态圆图标（`FontIcon` F136+F13F）+ 自动换行 `TextBlock` + 右侧 `HyperlinkButton` | 替代原工具的 Label + LinkLabel；a29e064 的 `InfoBar` 没有 Content/ActionButton，链接放不进同一行，故自绘 |
| 工具栏 | `CommandBar` + `AppBarButton`（`SymbolIcon`: Refresh / SelectAll / ClearSelection / Save）+ `AppBarSeparator` | 保存按钮用强调样式；禁用规则见 §3 |
| 计数 | `TextBlock` | "已豁免 n / 共 m"（已定保留）。n 按当前勾选计算，脏时同步变化 |
| 表头 | `Grid` 一行，每列一个无边框 `Button`，排序列显示 ▲/▼ | reactor 无 DataGrid，用固定列宽 `Grid` 自绘 |
| 列表 | `ListView` + `VirtualSource`，行模板为与表头同列宽的 `Grid`：`CheckBox` + 7 个 `TextBlock`（单行、尾部省略、`tooltip` 显示全文） | 248 行级别数据需虚拟化 |
| 状态栏 | 底部 `Border` + `TextBlock`；刷新中显示 `ProgressRing`（小号） | |
| 右键菜单 | `MenuExt::menu` 挂在行上 | 项目见 §3.6 |
| 确认/错误/孤儿 | `ContentDialog` | §4.4 |

列宽（DIP，初始值）：勾选 40、显示名称 220、描述 220、包 200、AC 名称 160、AC SID 140、用户 140、文件 *（剩余）。

### 4.3 状态与按钮可用性

| 状态 | 刷新 | 全部豁免/取消 | 保存 | 列表 |
| --- | --- | --- | --- | --- |
| 刷新中 | 禁用 | 禁用 | 禁用 | 禁用 + ProgressRing |
| 已加载、干净 | 可用 | 列表非空时可用 | 禁用 | 可用 |
| 已加载、脏 | 可用（需确认） | 可用 | 可用 | 可用 |
| 加载失败 | 可用 | 禁用 | 禁用 | 禁用 |

### 4.4 对话框

| 对话框 | 标题 | 正文 | 按钮（默认） |
| --- | --- | --- | --- |
| 刷新丢弃修改 | 放弃修改？ | 尚未保存修改，仍要刷新吗？ | 刷新 / **取消** |
| 退出丢弃修改 | 不保存就退出？ | 尚未保存修改，仍要退出吗？ | 退出 / **取消** |
| 孤儿记录 | 发现孤立的豁免记录 | 存在 SID 为 `{sid}`（可选择文本）的回环豁免，但找不到对应的 AppContainer。 + 复选框"本次会话中继续提示其他孤立记录"（默认勾选） | **确定** |
| 需要管理员 | 未保存修改 | 必须以管理员身份运行本工具才能修改 AppContainer 配置。 | 确定 |
| 防火墙停用 | Windows 防火墙已停用 | 见 §3.7 | 确定 |
| 通用错误 | 操作失败 / 保存失败 | 详情 | 确定 |

孤儿对话框把原工具的"是/否"问句改成复选框，语义不变（取消勾选 = 原工具选"否"）。

### 4.5 键盘

| 键 | 作用 |
| --- | --- |
| `F5` | 刷新（原工具为 `Alt+R` 助记符；WinUI 用 `AccessKey`，`Alt` 后显示 R/A/N/S/L） |
| `Ctrl+S` | 保存（新增，替代 `Alt+S` 助记符） |
| `Ctrl+A` | 列表全选 |
| `Ctrl+C` | 复制选中行 |
| `Space` | 切换焦点行勾选（ListView 默认） |
| `F1` / `Alt+L` | 了解更多 |

## 5. i18n

- 语言：与主应用一致的 `en`、`zh-cn`、`zh-tw`、`ja`、`ko`、`ru`。
- 方案：`rust-i18n`（主仓 `backend/tauri` 已用 v4），文案放 `crates/enable-loopback/locales/*.json`，键按 `window.*`、`toolbar.*`、`column.*`、`status.*`、`dialog.*`、`cli.*`、`placeholder.*` 分组。
- 语言选择顺序：`--lang <code>` 参数（主仓调用时传入当前应用语言）> 系统 UI 语言（`sys-locale`，主仓 `nyanpasu-helper` 已用）> `en`。
- 不翻译：`LoopbackExempt`/`LoopbackBlocked`、SID、包名、错误码。
- 时间格式：状态栏时间用系统长时间格式（`GetTimeFormatEx`），与语言设置一致。
- 主仓需同步：`win_uwp.rs` 调用时追加 `--lang`。

## 6. windows-reactor 能力核对与缺口

核对对象为 crates.io `windows-reactor 0.100.0` 与 windows-rs master `a29e064`（2026-10-19）源码。

| 需求 | 0.100.0 | master `a29e064` | 处理 |
| --- | --- | --- | --- |
| 运行时依赖 | 框架依赖：要求已安装 **Windows App Runtime 2.4**（`Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe`），缺失时弹英文 MessageBox 引导下载 | 同左；另有 `windows-reactor-setup`（crates.io 0.100.0）支持自包含部署 | 见 §8.1 与 §9-1 |
| 表格 | 无 DataGrid | 同左 | 自绘表头 + 固定列宽；不支持拖动调列宽 |
| 多选 | `ListView` 有 `Multiple`/`Extended` 模式，但 `on_selection_changed` 只回传 `Option<usize>` | 同左；但 `Border` 新增带修饰键的指针与键盘事件 | 拿不到完整选中集合，上游无相关 issue（2026-10-19 检索）。自建选中模型，见 §6.2 |
| 右键菜单 | `MenuExt::menu`，回调参数为菜单项文本 | `menu`，回调参数为 `Key` | 可用；"复制选中列"需要知道右键所在列：在每个单元格分别挂菜单 |
| 剪贴板 | 无 | 无 | 直接用 Win32 剪贴板 API（`OpenClipboard`/`SetClipboardData(CF_UNICODETEXT)`） |
| 关闭拦截 | 只有 `WindowRef::request_close`，无 closing 回调 | 仍无 closing 回调；但新增 `ComponentContext::run_window`，在 UI 线程把窗口 HWND（`IWindowNative::WindowHandle`）交给回调，属公开的原生集成入口 | 用 `run_window` 取 HWND 后 `SetWindowSubclass` 拦截 `WM_CLOSE`，见 §6.1 |
| 单实例置前 | 无 | 无 | 互斥量 + 共享内存中的 HWND + `SetForegroundWindow`，与 UI 框架无关 |
| 后台任务 | `spawn_background` | 同左 | 刷新用 |
| 对话框 | `ContentDialog` | 同左 | 可用 |
| 快捷键 | `KeyAccelerators` | 同左（D/O/S/M 键与组合修饰键缺失，见上游 #5087 第 14 项） | `Ctrl+S` 受影响：用 HWND 子类化处理 `WM_KEYDOWN`，或等上游 |

**版本选择**：关闭拦截依赖 master 才有的 `run_window`，因此依赖 `windows-reactor = { git = "https://github.com/microsoft/windows-rs", rev = "<固定提交>" }`，下一个 crates.io 版本发布后改回版本号。不用第三方转发的 `windows-reactor-pre`（描述为"为 guinea 项目转发"，非官方）。

上游相关 issue（2026-10-19）：#5087（open，原生 UI 契约缺口，含上下文菜单、快捷键、窗口集成、ListView 容器样式；不含多选）；#4901（closed，窗口关闭与应用退出分离）。

### 6.1 退出前确认

1. 窗口创建后，组件在 `create` 中调用 `run_window`，对 HWND 安装子类过程，同时把 HWND 写入单实例共享内存。
2. 子类过程收到 `WM_CLOSE`：若组件状态为脏（共享的 `Rc<Cell<bool>>`，UI 线程内读写），吞掉消息并给组件发消息"请求关闭"；否则交给默认处理。
3. 组件弹"不保存就退出？"`ContentDialog`；用户选"退出"后置位"已确认"标志并调用 `close_window`，子类过程见到标志即放行。
4. 风险：需要实测 WinUI 3 的标题栏关闭按钮与 `Alt+F4` 都经过 `WM_CLOSE`（AppWindow 的 Closing 事件即由此触发，预期成立）。

### 6.2 自建多选模型

master 的 `Border` 提供 `on_pointer_pressed`（`PointerEventInfo` 含 `modifiers`：Shift/Control，及左右键）与 `on_key_down`/`on_key_up`（`KeyEventInfo` 含按键与修饰键），足以在应用内实现多选，不必改 reactor：

1. `ListView` 设为 `Single`，WinUI 的单选索引作为**焦点行**（负责键盘焦点与滚动到可见），由 `on_selection_changed` 回报。
2. 应用自己维护 `selected: BTreeSet<Sid>` 与 `anchor`。每行包一层 `Border`，`on_pointer_pressed`：
   - 无修饰键左键：选中集合 = {该行}，`anchor` = 该行；
   - `Ctrl`：切换该行；
   - `Shift`：选中 `anchor`..该行（按当前排序后的顺序）；
   - 右键：该行不在集合内时先改为单选该行（与 Windows 资源管理器一致），并记录右键所在列供"复制选中列"使用。
3. 列表外层 `Border` 的 `on_key_down`：`Shift+↑/↓` 扩展选区；`Ctrl+A` 全选；`Ctrl+C` 复制；单独 `↑/↓` 交给 WinUI 移动焦点行后同步为单选。
4. 选中行用主题的选中底色绘制（行 `Border` 背景），WinUI 自带的单选高亮通过 ListView 容器样式关闭或与之一致（实施时确认；容器样式能力见上游 #5087 第 8 项）。
5. 选中集合按 SID 保存，刷新与排序后仍然有效；刷新后不存在的 SID 自动移出。
6. 纯逻辑（点击/修饰键/区间计算）放在 `model`，用普通值测试。

风险：ListViewItem 自身也处理指针按下，需要实测行内 `Border` 的 `on_pointer_pressed` 在其之前触发且修饰键可靠。若不成立，退路是给 reactor 打补丁（`[patch]` 指向我们 fork 的 windows-rs 固定提交），为 `ListView` 增加选中集合回调，并同时向上游提 PR。

## 7. 测试

- `model`：脏检测、全选/全不选的状态文案、排序（序数、升降切换）、复制行/列格式（单行无换行、多行 `\r\n`）、`-allow`/`-deny` 过滤规则、参数解析（含 `-allow` 优先于 `-all`、NAME 含空格）、按 SID 合并用户、占位文案与空串差别、孤儿检测。全部用普通值测试。
- `netiso`：真实 API 只做冒烟（读取枚举与豁免名单不需提权，可在 CI 的 Windows runner 跑只读测试）；写入不在 CI 做。
- i18n：所有语言键集合一致的测试。
- 手动冒烟：提权/非提权保存、孤儿记录（可用 `CheckNetIsolation LoopbackExempt -a -p=<不存在的 SID>` 构造）、防火墙服务停止时的 1753 提示、`-registry`、`manual`、单实例置前、命令行各模式、各语言显示。

## 8. 主仓接入

1. `.gitmodules` 新增 `backend/enable-loopback` → `https://github.com/libnyanpasu/enable-loopback.git`；`backend/Cargo.toml` 的 `exclude` 加入该路径。
2. 取得产物沿用 nyanpasu-service 的做法（`scripts/src/prepare/binaries.ts` 的 `getNyanpasuServiceInfo`）：从 submodule 的 `crates/enable-loopback/Cargo.toml` 读版本号，下载子仓**同版本 tag** 的 release 产物，并对照 release 附带的 SHA-256 校验。版本随 submodule 指针一起变化，消除当前 `latest` 无校验的供应链风险。不在主仓构建流程里编译子仓，避免主仓 CI 多编一份 windows-rs。
3. `scripts/src/prepare/check.ts` 删掉 `Kuingsmile/uwp-tool` 的下载项，改走上一条。
4. 架构：子仓发布 x64 与 arm64，按主仓打包目标选择。
5. `win_uwp.rs` 追加 `--lang <当前应用语言>`。
6. 子仓 CI：Windows runner 上 `cargo test`、clippy、fmt，tag 时构建两种架构并上传产物与校验和。

### 8.1 Windows App Runtime 的获取方式（调研结果）

| 方式 | 情况 |
| --- | --- |
| 微软商店 | **没有面向用户的独立商店条目**（`winget search "Windows App Runtime" --source msstore` 无结果）。商店应用依赖它时由商店自动安装框架包，所以装过较新商店应用的机器上往往已有（本机已有 2.2–2.5），但不能假定 |
| 官方安装器 | `https://aka.ms/windowsappsdk/2.4/2.4.0/windowsappruntimeinstall-x64.exe`（及 arm64），支持离线分发，需要管理员权限；下载页 `https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads` |
| winget | `winget install Microsoft.WindowsAppRuntime.2`（2026-10-19 最高 2.4.0，落后于下载页的 2.5.1） |
| 自包含 | `windows-reactor-setup` 在构建时从 NuGet 拉取固定版本的 runtime，把所需 DLL 和目录复制到 exe 旁并嵌入自包含清单；用户机器无需安装任何东西，代价是安装包变大（数十 MB 量级，需实测） |

### 8.2 部署决定：用户自行安装（框架依赖）

1. 子仓不用 `windows-reactor-setup`，exe 依赖已安装的 `Microsoft.WindowsAppRuntime.2` 框架包，版本下限等于所用 reactor 提交的 `WINDOWSAPPSDK_RUNTIME_VERSION`：0.100.0 为 2.4.0，锁定的 master `a29e064` 为 **2.5.1**（`crates/libs/reactor/src/native/bindings.rs`）。升级 reactor 提交时同步核对。
2. 主仓在 `win_uwp.rs` 启动工具前检测：`GetPackagesByPackageFamily("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe")`，并检查版本 ≥ 下限、架构匹配。此检测是 Windows 平台适配，放在现有 `win_uwp.rs` 旁，不进入 core。
3. 缺失时前端弹本地化对话框，提供：
   - "下载安装程序"：直接打开与所需版本和架构匹配的官方安装器链接 `https://aka.ms/windowsappsdk/<major.minor>/<version>/windowsappruntimeinstall-<x64|arm64>.exe`（锁定 `a29e064` 时为 `2.5/2.5.1`）；
   - "打开下载页"：`https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads`；
   - "取消"。
   **不提供 winget 安装**：2026-10-19 winget 上 `Microsoft.WindowsAppRuntime.2` 最高只有 2.4.0，低于所需的 2.5.1，装完仍无法启动。winget 跟上后可再评估。
   文案进主前端的 paraglide 消息（六语言）。安装完成后用户再次点击即可。
4. 工具自身的兜底：若用户绕过主仓直接运行且缺失 runtime，reactor 的 bootstrap 会弹英文提示框。子仓在调用 reactor 前自行检测，缺失时弹本地化对话框（`TaskDialogIndirect`，按钮同第 3 条，不依赖 WinUI），避免出现英文提示；检测接口本身失败时只记调试日志并继续交给 reactor。命令行模式不检测（不需要 runtime）。

## 9. 待决问题

已定（2026-10-19）：submodule；修正保存缺陷；链接到微软文档；标题本地化；保留计数；不做 x86；runtime 由用户自行安装（框架依赖，主仓检测并本地化引导，见 §8.2）；接受 git rev 依赖 windows-rs master；多选自建（§6.2）；图标新绘、Fluent、SVG。

1. 图标图形：待从 codex 概念图中选定。
