# Wattson Native egui + wgpu GUI Design

**Date / 日期**: 2026-03-26
**Issue / 议题**: [#12](https://github.com/exomind-team/wattson/issues/12)

## Context / 背景

`wattson` 目前是一个以 CLI/TUI/API 为主的 Rust 电源监控工具，已有这些稳定能力：

- `read` / `watch` / `info` / `cost` / `ports`
- `config` 配置管理
- `tui` 终端仪表盘
- `serve` HTTP API
- `chart` PNG 图表导出

用户需求不是“再加一个窗口”，而是把它升级成以 `wgpu + egui` 为核心渲染路径的原生桌面电源软件，同时保留原有功能，并新增图形控制设置、亮暗色主题和端到端测试。

## References / 参考

- 日记 `2026-03-25.md` 记录了 “GUI 设计方案（egui+eframe，参考 ALIEN）”
- 知识库《工程实践-D0虚拟机Rust架构》给出的可视化对比是：
  - `egui (eframe)`：纯 Rust、即时模式 GUI、适合交互式调试
  - `wgpu + 自定义渲染`：性能最强，但开发成本高
- `eframe` 官方文档说明 `NativeOptions` 可直接指定 `renderer` 与 `wgpu_options`
- `egui::ThemePreference` 官方文档支持 `Dark / Light / System`
- `egui_kittest` 官方文档支持在启用 `wgpu + snapshot` 时进行 UI 快照测试

## Design Goals / 设计目标

1. 用 `eframe` 提供基于 `wgpu` 的原生窗口渲染主路径。
2. 保留现有 CLI/TUI/API/chart/history/config 行为，不回退已有能力。
3. GUI 内直接控制：
   - 主题 Theme（Dark / Light / System，深色 / 浅色 / 跟随系统）
   - UI 刷新 FPS（帧率 / refresh）
   - 串口轮询间隔 poll interval（轮询间隔）
   - 图表时间窗 time window（时间窗口）
   - AC/DC 曲线显隐 visibility（可见性）
   - 图表自动缩放/零基线 scale（缩放）
4. 设置持久化，重启后恢复。
5. 支持真实设备和 demo 数据，两条路径都可验证。
6. 能自动化测试 GUI 状态与 demo 端到端截图。

## Approaches / 方案比较

### Approach A / 方案 A：新增 GUI，保留 CLI/TUI/API，并让桌面入口 GUI-first

- 做法：引入 `eframe`，新增 `gui` 子命令，同时允许无子命令时默认进入 GUI。
- 优点：
  - 满足“软件变成原生渲染 GUI”的目标
  - 不破坏现有脚本化/终端/API 使用场景
  - 基于 `eframe` 直接获得 `wgpu` 原生加速和窗口持久化
  - 测试成本可控，YAGNI
- 缺点：
  - 代码面会同时维护 TUI 与 GUI

### Approach B / 方案 B：完全替换 TUI，所有交互迁移到 GUI

- 做法：删除 `ratatui` 路径，把所有交互收敛到 GUI。
- 优点：
  - UI 路径单一
  - 用户侧认知更简单
- 缺点：
  - 不满足“原来的功能全部保留”
  - 会损失 SSH / 纯终端使用场景
  - 回归风险大

### Approach C / 方案 C：自写低层 `wgpu` 渲染器，再叠加 `egui`

- 做法：图表和面板用原生 `wgpu` 自绘，设置区用 `egui`。
- 优点：
  - 理论性能最高
  - 可以做非常定制的可视化
- 缺点：
  - 对当前 3~10Hz 电源遥测属于明显过度设计
  - 实现、调试、截图测试都显著更难
  - 不能在本轮里高质量完成“保留全部功能 + 测试 + PR”

## Recommendation / 推荐

采用 **方案 A**。

核心理由：

- 当前数据吞吐并不大，瓶颈不是数据量，而是 UI 体验与刷新流畅度。
- `eframe` 已经使用 `wgpu` 作为原生渲染后端，可直接满足“用好 `wgpu + egui`”而不是空喊概念。
- 保留现有命令面，能避免功能回退。
- `egui_kittest` 能提供可执行的 UI 快照测试，这对“全部做好测试再审查”更重要。

## Architecture / 架构设计

### 1. App layering / 分层

- `serial` 仍负责真实设备采集
- 新增 `app` / `runtime` 层，统一管理：
  - 实时快照 snapshot（实时数据）
  - 历史 ring buffer（环形历史）
  - 统计 statistics（统计）
  - GUI settings（图形设置）
  - data source（真实串口 / demo）
- `tui` 继续复用底层数据结构，不再承担唯一可视化职责
- 新增 `gui` 模块，负责 `egui` 视图组合和用户交互

### 2. GUI surface / GUI 组成

- Top bar 顶栏：
  - 连接状态、设备型号、串口、主题切换、开始/停止、数据源切换
- Left panel 左侧摘要：
  - AC 输入、DC 输出、效率、温度、风扇、总耗电、总费用
- Center panel 中央图表：
  - AC / DC 双曲线
  - 时间窗口、缩放模式、曲线显隐
- Right panel 右侧设置：
  - Theme 主题
  - FPS/refresh
  - Poll interval
  - Window size / persistence summary（窗口/设置持久化摘要）
  - Chart control（图表控制）
- Bottom status 底栏：
  - 数据龄期 data age
  - packet/error count
  - API/TUI/History 状态提示

### 3. Persistence / 持久化

- 现有 `wattson.toml` 继续保存设备与成本等核心配置
- 新增 GUI settings 段或独立 GUI state 文件，保存：
  - theme_preference
  - chart_time_window_s
  - show_ac / show_dc
  - chart_scale_mode
  - ui_refresh_ms
  - preferred_window_size
- 保留 `wattson_history.json`

### 4. Demo mode / 演示模式

- 新增 demo 数据源，用固定脚本/波形产生可重复 snapshot
- GUI 可无硬件启动：
  - 本地开发不依赖真实电源
  - 快照测试和截图测试稳定
  - 端到端测试可以覆盖完整窗口路径

### 5. Backward compatibility / 向后兼容

- 原子命令全部保留：
  - `read/watch/config/tui/serve/chart/info/cost/ports`
- 新增：
  - `gui`
  - `gui --demo`
  - `gui --screenshot <path>` for automated capture（自动截图）
- 可以让无子命令默认进入 GUI，但所有旧子命令保持不变

## Testing Strategy / 测试策略

### Unit tests / 单元测试

- GUI settings 序列化/反序列化
- ring buffer 时间窗裁剪
- stats 计算与图表可见性逻辑
- demo 数据生成稳定性

### Integration tests / 集成测试

- `cargo test` 覆盖 config 与 app state
- `gui --demo --screenshot <path>` 生成 PNG，验证文件存在与尺寸
- CLI 默认启动路径与 `gui` 子命令参数解析

### UI snapshot tests / UI 快照测试

- 通过 `egui_kittest` 渲染关键界面状态
- 覆盖亮色与暗色两套主题

## Risks / 风险

- Windows 上 `eframe` 依赖较多，首次编译会明显变慢
- UI 自动化截图若绑定真实 GPU/驱动状态，可能不稳定
- 如果直接把真实串口句柄塞进 GUI 线程，测试性会变差

## Mitigations / 缓解

- 采用 source trait / 数据源 trait，真实串口与 demo 同一接口
- GUI screenshot 走 demo 数据，避免硬件依赖
- 保持 TUI 路径可用，作为回退与对照验证

## Success Criteria / 完成标准

- GUI 使用 `eframe` 的 `wgpu` 渲染后端启动成功
- 亮色 / 暗色 / 跟随系统三种主题可选并持久化
- 原有命令保持可用
- 自动测试覆盖 GUI 状态与 demo 截图流程
- README、PR、日记都有对应记录
