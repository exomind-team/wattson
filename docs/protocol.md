# DM-1000G / KE Serial Protocol Notes

本文档记录 `Segotep / 鑫谷` 数字电源串口协议中，`DM-1000G(D)` 当前已经确认的读取与风扇写入部分。

## Frame Layout / 帧结构

当前高置信度帧格式：

```text
55 7E LEN CMD DATA... CHECKSUM AE
```

- `55 7E`: fixed header / 固定帧头
- `LEN`: byte count from `CMD` to `CHECKSUM` inclusive
- `CMD`: command / packet type
- `DATA`: payload / 载荷
- `CHECKSUM`: one-byte checksum / 1 字节校验
- `AE`: fixed footer / 固定帧尾

### Example / 例子

Active query / 主动查询：

```text
55 7E 02 04 06 AE
```

- `LEN=02`
- `CMD=04`
- no payload / 无数据区
- `CHECKSUM=06`

## Read Commands / 读取命令

已确认读取包：

| CMD | Meaning / 含义 | Notes / 备注 |
|-----|----------------|--------------|
| `0x02` | Electrical telemetry / 电气遥测 | little-endian `u16`, includes fan RPM raw |
| `0x03` | Model packet / 型号包 | contains model string, may also contain serial marker |
| `0x04` | Extended status / 扩展状态 | big-endian `u16`, includes temp + AC power + fan-related status byte |
| `0x05` | Serial number / 序列号 | ASCII serial text |

当前 `wattson` 的读取实现已稳定支持：

- `0x02`: `12V/5V/3.3V`, currents, AC voltage/frequency, fan RPM
- `0x04`: main temp, air temps, AC input power
- `0x03` / `0x05`: model + serial

## Write Commands / 写入命令

### `0x13` Fan Mode / 风扇模式

来源：

- `Segotep-PSU-Toolbox/鑫谷KE1300通讯.xlsx`
- HiMOS 前端枚举值
- DM-1000G 本机 `COM4` 探针

模式值：

| Mode | Value | Frame |
|------|-------|-------|
| `Auto / 自动` | `00` | `55 7E 04 13 00 00 17 AE` |
| `Silent / 静音` | `01` | `55 7E 04 13 00 01 18 AE` |
| `Performance / 超频` | `02` | `55 7E 04 13 00 02 19 AE` |
| `Custom / 自定义` | `03` | `55 7E 04 13 00 03 20 AE` |
| `Clean / 清灰` | `04` | `55 7E 04 13 00 04 21 AE` |

说明：

- 前三个模式的校验字节与简单求和一致。
- `Custom` / `Clean` 的校验字节以协议表字面值 `20` / `21` 为准。
- 当前实现按协议表直接构造 `0x13` 短帧，不再猜测通用校验算法。

### `0x1B` Custom Fan Curve / 自定义风扇曲线

来源：

- `Segotep-PSU-Toolbox/鑫谷KE1300通讯.xlsx`
- HiMOS 前端 `powerSupplyFanSet(sn, JSON.stringify({ mode, custom }))`
- DM-1000G 本机 `COM4` 手工发帧探针

#### Payload layout / 载荷布局

`LEN=1D`, `CMD=1B`

- data byte `1..21`: temperatures `0C, 5C, 10C, ... 100C` 的风扇百分比
- data byte `22..27`: 3 个可拖拽控制点
  - `22`: point1 temp
  - `23`: point1 pwm
  - `24`: point2 temp
  - `25`: point2 pwm
  - `26`: point3 temp
  - `27`: point3 pwm
- byte `28`: CRC-8

#### Checksum / 校验

`0x1B` 长帧使用标准 `CRC-8`：

- polynomial / 多项式: `0x07`
- init / 初值: `0x00`
- input range / 计算区间: from `LEN` to the last data byte

即：

```text
CRC8(1D 1B ... data_byte_27)
```

证据：

- HiMOS 包依赖 npm `crc`
- 主进程字节码可搜到 `crc8`
- 本地重放样例与实机状态变化匹配

#### Hardware-verified sample / 实机已验证样例

平直 `80%` 曲线 + `Custom` 模式：

```text
Curve frame:
55 7E 1D 1B
50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50 50
1E 50 3C 50 5A 50
63 AE

Mode frame:
55 7E 04 13 00 03 20 AE
```

控制点解释：

- `1E 50` => `30C @ 80%`
- `3C 50` => `60C @ 80%`
- `5A 50` => `90C @ 80%`
- `63` => `CRC8(1D 1B ... 5A 50)`

### Curve Model Used By Wattson / Wattson 当前采用的曲线模型

为了兼容 HiMOS 的 3 个可拖拽点模型，`wattson` 当前支持两种输入：

- `3` points: interpreted as interior control points / 解释为中间 3 个控制点，自动补 `0C@0%` 与 `100C@100%`
- `5` points: full curve / 完整 5 点曲线，必须以 `0C` 开头、`100C` 结尾

序列化时会：

1. 将点按温度升序校验
2. 线性插值展开为 `21` 个 `5C` 采样点
3. 附上 `3` 个中间控制点
4. 计算 `CRC-8`

`set_fan_pwm(value)` 当前通过“平直曲线 + `Custom` 模式”实现，而不是独立的固定 PWM opcode。

## Capture Evidence / 抓包与静态证据

关键参考：

- `D:\project\_tmp_wattson_refs\Segotep-PSU-Toolbox\鑫谷KE1300通讯.xlsx`
- `D:\project\_tmp_wattson_refs\himos_asar\out\renderer\assets\Index-38349036.js`
- `D:\project\_tmp_wattson_refs\himos_asar\node_modules\crc\cjs\calculators\crc8.js`

HiMOS 前端确认的模式枚举：

- `Auto = 0`
- `Silent = 1`
- `Performance = 2`
- `Custom = 3`
- `Clean = 4`

HiMOS 前端调用链：

- `powerSupplyFanModeSet(sn, JSON.stringify({ mode, custom }))`
- `powerSupplyFanSet(sn, JSON.stringify({ mode, custom }))`

## Known Gaps / 已知未解问题

以下内容仍未完全锁定：

- `0x04` 包中风扇相关状态位的精确语义
- 写入后 `fan.rpm` 在部分轮询中读成 `0`，是读取字段解释偏差还是设备状态切换后的另一种编码，仍需继续抓包确认
- `0x13` 模式帧是否还有可推广的统一校验规则，目前仅确认表格列出的 5 个字面值

## Practical Notes / 实操备注

- 写命令必须复用同一个后台串口线程，不要第二次打开 `COM4`
- 当前 `wattson` 的写顺序：
  - `set_fan_mode(mode)` => send `0x13`
  - `set_fan_curve(points)` => send `0x1B`, then `0x13 Custom`
  - `set_fan_pwm(value)` => synthesize flat curve, then `0x13 Custom`
- 每次写完后会再发一次查询帧，尽快刷新遥测
