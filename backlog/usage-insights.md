# 用量洞察

状态:**第 1 期已实现**(2026-11-22,见 CHANGELOG「未发布」);第 2、3 期未开工。记录于 2026-11-22,同日修订(见文末「修订记录」)。

## 要解决什么

在窗格里跑 Claude Code / Codex 时,用户不知道 token 花在了哪里、哪些习惯在白白多花。
这个功能分析用量记录,给出能省 token 的具体建议,并提供按厂家 / 模型、项目、会话查看用量和耗时的面板,
以及技能、子代理各自触发了多少次、用了哪些模型和多少 token。

## 方案

```
~/.claude/projects/**/*.jsonl ─┐
                               ├─> 适配器 ──> 扫描(游标 + 去重)──> 台账 ──> 规则 ──> 面板 / 状态栏提示
~/.codex/sessions/**/*.jsonl  ─┘                                     ↑
                                    窗格橙灯时长(运行时单独记)──────┘
```

- **只读本地记录**:不额外调 API,不需要改 CLI 的配置,数据不离开本机。
- **只用规则,不用 LLM 分析**:为了省 token 再花 token 不划算,下面的习惯都能确定性地算出来。
- **现有的状态栏用量条不经过台账**:它仍由 `claude_usage.rs` 每 2 秒增量读取,台账只服务面板和建议。

## 数据源

以下字段在 2026-11-21 对本机记录核对过(Claude Code 2.1.330、Codex CLI 0.171.0)。

| 指标 | Claude Code | Codex |
|---|---|---|
| 文件位置 | `~/.claude/projects/<cwd 换成 ->/<会话 id>.jsonl` | `~/.codex/sessions/年/月/日/rollout-*.jsonl`,项目要读 `session_meta.cwd` |
| token 用量 | assistant 记录的 `message.usage`;`input_tokens` **不含**缓存读写 | `token_count` 事件的 `last_token_usage` / `total_token_usage`,每轮另有 `token_usage_record`(带 `turn_id`);`input_tokens` **已含** `cached_input_tokens`,`output_tokens` **已含** `reasoning_output_tokens`,`total_tokens` = 输入 + 输出 |
| 缓存 TTL | `usage.cache_creation` 分 `ephemeral_5m` / `ephemeral_1h` | 有 `cached_input_tokens` 和 `cache_write_input_tokens`(本机记录里始终为 0),没有 TTL |
| 上下文窗口 | 模型名带 `[1m]` 为 1M,否则 200K | `model_context_window` 直接给出 |
| 每轮耗时 | `system/turn_duration` 的 `durationMs`,**不是每轮都有**,缺失时用时间戳差 | `task_complete` 的 `duration_ms`,每轮都有 |
| 思考时长 | assistant 记录的 `thinkingDurationMs` | — |
| 工具输出 | user 记录的 `toolUseResult` | `custom_tool_call_output` |
| 套餐额度 | — | `rate_limits`:5 小时窗口和周窗口的 `used_percent`、`resets_at` |
| 会话汇总 | `cost-state`:API 耗时、工具耗时、费用 | — |
| 技能触发 | assistant 记录里 `name: "Skill"` 的 `tool_use`,技能名在 `input.skill`(结果里是 `toolUseResult.commandName`) | 没有专门的工具:模型用 shell 读 `SKILL.md`。认 `item_completed` 里 `CommandExecution` 的 `parsed_cmd` 中 `type: "read"`、路径以 `SKILL.md` 结尾的项;技能名取路径里 `skills/<名>/` 一段,加载体积取 `aggregated_output` 字节数。技能清单在 `world_state.state.host_skills`。来源有 `~/.codex/skills/`(含 `.system/`)和 `~/.codex/plugins/cache/<源>/<插件>/<版本>/skills/` |
| 子代理 | `name: "Agent"` 的 `tool_use`,类型在 `input.subagent_type`;它自己的对话在 `<会话 id>/subagents/agent-<id>.jsonl`,每条 assistant 记录带 `message.model` 和 `message.usage`;同名 `.meta.json` 里有 `agentType`、`toolUseId` | 子代理是**独立的 rollout 文件**,`session_meta` 里有 `parent_thread_id`,`source` 为 `{"subagent": {...}}`、`thread_source` 标明用途;模型在它自己的 `turn_context.model`,用量在它自己的 `token_count`。本机只见到自动审批用的 `guardian`(`thread_source: guardian_review`,模型 `codex-auto-review`);`collaboration.spawn_agent` 起的子代理工具已在清单里,但本机还没有实际记录,字段待核对 |
| 进程识别 | 前台进程路径含 `claude`(已有) | 前台进程路径含 `codex`(待加) |

## 能识别的习惯和建议

| 规则 | 判定 | 建议 | 适用 |
|---|---|---|---|
| 缓存过期后续聊 | 请求间隔超过 TTL,下一条 `cache_read` 很小、`cache_creation` 很大 | 离开前 `/compact`,或回来后 `/clear` | Claude |
| 起步上下文偏重 | 会话第一条请求的 token 数 | 结合加载器清单,停用用不到的技能 / 插件 | 两者 |
| 长会话不清理 | 轮数、平均每轮上下文、自动压缩次数 | 换任务时清理,不等自动压缩 | 两者 |
| 工具输出过大 | 按工具汇总输出体积 | 列出最大的几项,建议分段读或截尾 | 两者 |
| 重复读同一文件 | 按路径统计读取次数 | 多为压缩后重读,会话该拆了 | 两者 |
| 大模型做小事 | 无工具调用且输出很短的轮次占比,按模型分 | 换小模型 / 调低推理强度 | 两者 |
| 又慢又贵的轮次 | 耗时和 token 都在前 10% | 列出它在做什么 | 两者 |
| 工具耗时占比高 | 时间主要花在命令上而不是模型上 | 优化命令本身(增量构建、只跑相关测试) | 两者 |
| 等确认时间长 | 橙灯总时长 | 把常用只读命令加进允许列表 | Claude(Codex 调 approval 策略) |
| 额度接近上限 | `rate_limits.used_percent` | 提示剩余额度和重置时间 | Codex |

用量只用两个口径,不再折算"等效 token":

- **Token 总量**:输入、缓存写、缓存读、输出四项原始数量直接相加。缓存读通常占绝大部分,所以面板同时给出分项。
  两家字段的包含关系不同,Codex 不能照搬这个加法,见「台账」一节的口径统一。
- **折合费用**:四项各自乘以该模型的单价再相加。单价表内置、可在配置里覆盖;缓存写按 5 分钟 / 1 小时两档分别计价。
  表里没有的模型费用显示"—",不拿别的模型的价格顶替。内置单价实现时要对着官方价目核对,不凭印象写。
  订阅套餐不按 token 计费,这个数只用来比较轻重,界面上写"折合费用"而不是"花费"。

建议的节省量按折合费用估算和排序(token 数会被便宜的缓存读撑大,不适合用来比较)。

规则文案里的斜杠命令名由适配器提供。Codex 的命令名(`/new`、`/compact`)和缓存机制是凭印象写的,实现前要对着 `codex` 的帮助核对。

## 台账

位置 `~/.config/voiceghostty/usage/`,按月一个文件,每轮对话一行,只存数字,不存对话内容:

```
时间, agent, 项目, 会话 id, 轮次序号, 模型, 输入, 缓存写, 缓存读, 输出, 耗时 ms, 思考 ms,
工具调用数, 最大工具输出字节, 等确认 ms, 子代理类型, 子代理 id, 触发的技能, 技能加载字节
```

- **子代理的用量单独成行**:子代理的对话不在主会话文件里(Claude 在 `<会话 id>/subagents/` 下,Codex 是另一个 rollout 文件),扫描时要一并读,否则总量会少算。
  这些行的"子代理类型 / 子代理 id"非空,"会话 id"填父会话;主对话的行这两列为空。用量页的总量包含子代理,会话页把它们归到父会话下。
- **技能没有独立用量**:它只是把一段说明加载进主对话的上下文。台账在触发它的那一轮记下技能名和加载字节数,
  面板据此显示触发次数、加载体积(字节数折算的估算值)和触发时主对话的模型;后续消耗已经算在主对话里,不重复归给技能。
  技能内容具体以什么形式进入记录、用户手敲 `/技能名` 和模型自动触发能否区分,实现前要对着真实记录核对。
- **Codex 的两处差异**:
  - 子代理本身就是独立的 rollout 文件,扫描时按 `parent_thread_id` 归到父会话即可,不用另找目录。
  - 技能触发是从"读 `SKILL.md` 的命令"推断出来的:模型如果只读一部分(`sed -n`)、读多次或换别的方式读,计数会有偏差,
    同一会话同一技能只算一次触发。
- **口径统一**:Codex 的 `input_tokens` 已含缓存读、`output_tokens` 已含推理,算"Token 总量"时用 `total_tokens`,
  不能照 Claude 的方式把缓存读再加一遍;算费用时输入要拆成"未缓存部分 = input − cached"和"缓存读"分别计价。

- **为什么要自己存**:Claude Code 默认约 30 天后清理会话记录(`cleanupPeriodDays`),只读原始文件看不了更久的历史。
- **扫描是唯一的写入路径**。三个触发点:
  - 定时:应用运行期间每隔一个周期(默认 1 小时,可设)。
  - 启动:距上次成功扫描超过一个周期就立即补扫。
  - 打开面板:总是先扫一次。
- **补扫范围用游标定,不用时间范围定**:先用"修改时间晚于上次成功扫描"筛出文件,再从每个文件各自的字节游标接着读。
  会话可能跨过扫描时刻,系统时间可能被改,旧会话可能被恢复后继续写,按时间切会重复或漏掉边界上的轮次。
- **到期判断用墙上时钟**,不数监视线程的轮次(休眠时线程不跑)。
- **去重**:按"会话 id + 轮次序号"。先追加台账、再更新游标,中途崩溃最多导致重读。
- **橙灯时长单独记**:这是唯一只有运行时才知道的数据。橙灯结束时往一个小文件追加一行(会话 id、起止时间),扫描时并入对应轮次。
  不在 VoiceGhostty 窗格里跑的会话没有这项数据,面板显示"—"而不是 0。
- **首次全量扫描放后台线程**(本机现有约 730 个文件、2 GB 多),面板显示"正在整理历史记录 312 / 729"。
- **保留期**:默认永久,设置里给"清除用量数据"按钮。

## 界面

### 状态栏用量条(现有的 `claude_meter` 扩展)

```
claude ▓░░░░░░ 65K / 1M  7%   ⏱ 2:14
codex  ▓░░░░░░ 26K / 258K  10%   额度 5h 82% · 周 31%
```

- `⏱` 是缓存倒计时,只在 Claude 窗格转绿、等回复时出现;剩 1 分钟内变橙,过期后显示"已过期"。
- Codex 不做倒计时,改显示额度。
- 悬停提示追加:上一轮耗时(思考 / 工具各多少)、本会话累计 token 总量和折合费用。
- 点击:按当前项目筛选并打开面板的会话页。

### 状态栏提示

复用"已上屏:…"的位置,几秒后自动消失,不弹窗、不响铃,同一条规则每个会话最多提示一次:

- "缓存已过期,下一句要重付 65K · 建议 /clear"
- "任务告一段落,上下文 52% · 可以 /compact"

末尾的命令可点,只敲进终端、不回车,与现有安全模型一致。

### 洞察面板

原型:[insights-panel-preview.html](insights-panel-preview.html)(本地打开,示例数据,配色按 `style.rs` 的 `Palette::of` 推导,可切四套主题)。

- 居中浮窗,宽 936、内容区高 520 可滚动,写法对照加载器(`kit::window` + `kit::window_header`)。应用窗口比它窄时面板要跟着收窄。
- **工具行**:四个页签(建议 / 用量 / 会话 / 技能与子代理,`kit::segmented`)、7 天 / 30 天、重新整理。
- **筛选条**:Agent、模型、项目,对所有页签同时生效。Agent 和项目是下拉框(`egui::ComboBox`)。
  - 模型是两级多选:厂家 → 模型。点开是一个带复选框的弹层,不选即全部;勾厂家等于勾它名下全部模型(部分勾选时厂家显示半选),
    也可以只勾一个或跨厂家勾多个。勾选后弹层不关,点外面或按 Esc 关闭。
  - 按钮文字:全部模型 / 单个模型名 / "<厂家> 全部" / "N 个模型"。
  - 选了 Agent 后弹层只列它能用的模型,已勾选里不属于它的自动去掉。
  - 厂家由适配器按模型名归类,认不出的归到"其他"。
- **建议页**:每条一张 `kit::card`,按预计节省量排序;含严重程度圆点、模型 / 项目标签、依据、建议、主动作按钮和"不再提示"。
- **用量页**:
  - 六个数字:Token 总量、折合费用、对话轮数、平均每轮、总等待时间、缓存命中率。
  - 每日堆叠柱状图,纵轴可切 Token / 费用,可切按类型(输入 / 缓存写 / 缓存读 / 输出)、按模型、按 agent;没有数据的日期画虚线,不画成零。
  - 模型表:按厂家分组,厂家行是小计、模型行缩进在下面;列为轮数、Token、费用、费用占比、平均每轮用量、平均每轮耗时、缓存命中率。
  - 项目表:Token、费用、轮数,按费用排序。点项目行按它筛选;点厂家行勾选整个厂家,点模型行把它加入或移出筛选。
- **会话页**:会话列表(开始时间、项目、agent、模型、轮数、Token、费用、峰值上下文、耗时),可按时间 / 用量 / 耗时排序;
  展开看每轮的耗时、思考时长、上下文增量、输出量、工具次数、最大工具输出、等确认时间。
- **技能与子代理页**(这里的"子代理"指 Claude Code 由 Agent 工具启动的 Explore、Plan 等,以及 Codex 的 guardian、spawn_agent 子代理;不是筛选条里的 Agent):
  - 六个数字:子代理启动次数、子代理 Token、子代理折合费用、占总费用比例、技能触发次数、技能累计加载。
  - 子代理表:按类型分组,展开到所用模型;列为次数、Token、费用、费用占比、每次 Token、每次耗时。
  - 技能表:按技能分组,展开到触发时主对话所用的模型;列为触发次数、会话数、每次加载、累计加载、加载占比。
  - 两张表的模型行可点,效果同模型表(加入或移出筛选)。表里带 agent 标签区分 claude / codex。
  - Codex 的技能触发是推断值,在悬停提示里注明。
- 图表用 `ui.painter()` 自绘,不引入新 crate。

### 触发入口

| 入口 | 行为 |
|---|---|
| 自动 | 监视线程读用量时顺带跑实时规则,命中就发状态栏提示 |
| 点用量条 | 打开面板的会话页,按当前项目筛选 |
| 快捷键 | <kbd>⌘I</kbd>(Linux / Windows <kbd>Ctrl+Shift+I</kbd>),`shortcuts.rs` 里目前未占用 |
| 语音命令 | 词表加"用量""省钱建议" |
| 状态栏图标 | 新增 `Icon::Chart`,放在加载器按钮旁 |

## 代码结构

| 位置 | 内容 |
|---|---|
| `vg-core/src/insights/mod.rs` | `TurnRecord`、`Summary`、`Advice`、`Agent { Claude, Codex }` |
| `vg-core/src/insights/ledger.rs` | 台账读写、游标、去重 |
| `vg-core/src/insights/scan.rs` | 扫描入口 |
| `vg-core/src/insights/claude.rs`、`codex.rs` | 两个适配器:找文件、解析成 `TurnRecord` |
| `vg-core/src/insights/rules.rs` | 规则:`Summary` → `Vec<Advice>`,纯函数 |
| `vg-core/src/insights/pricing.rs` | 单价表(按模型,分输入 / 缓存写 5 分钟 / 缓存写 1 小时 / 缓存读 / 输出)和厂家归类;可被配置覆盖 |
| `vg-core/src/insights/ext.rs` | 技能触发、子代理的汇总:`SkillEvent`、`SubagentRun`,供第四个页签用 |
| `vg-app/src/insights_panel.rs` | 面板界面,结构对照 `loader_panel.rs` |

```rust
pub struct Advice {
    pub rule: RuleId,           // 用于"不再提示"
    pub severity: Severity,
    pub agent: Agent,
    pub model: Option<String>,
    pub project: Option<PathBuf>,
    pub title: String,
    pub evidence: String,
    pub suggestion: String,
    pub saving_cost: Option<f64>, // 折合费用,排序依据;模型无单价时为 None,排在最后
    pub action: Option<AdviceAction>,
}
pub enum AdviceAction { TypeCommand(&'static str), OpenLoader, OpenSettings, Filter { .. } }
```

现有代码的改动点:

- `app.rs`:加 `insights` / `show_insights` 字段,`toggle_insights()`、`insights_window()` 对照加载器的写法。
- `shortcuts.rs`:加 `Action::Insights`。
- `chrome.rs`:`claude_meter` 改成可点击;状态栏加图标按钮。
- `widgets.rs`:用量条加倒计时 / 额度显示,支持 Codex。
- `voice_commands.rs`、`icons.rs`:加词条和图标。
- `settings.rs`:`insights_enabled`、`insights_hints`、`insights_muted_rules`、`insights_scan_minutes`、`insights_prices`(单价覆盖)。
- `monitor.rs`:定时触发扫描;扫描本身另起线程,不能卡住每 0.5 秒一轮的状态灯推进。
- `terminal/proc.rs`:进程识别加 `codex`。
- `paths.rs`:加 `usage_dir()`。

## 格式变动的防护

两份 JSONL 都是 CLI 的内部存档,不是公开接口,格式会随版本变动且不会通知。

- 宽松解析:每个字段都当可选,缺了就少显示一项,不报错。
- 每项指标带兜底(耗时用时间戳差,窗口大小用默认值)。
- 失效要看得见:文件在增长但连续解析不出用量时,用量条显示"格式不识别 · <版本>",不停在旧数字上。
- 用脱敏的真实记录做单测样本;适配器里记下验证过的最高版本,遇到更新的版本在悬停提示里注明"未验证"。
- 备选数据源:Claude Code 的状态行 / hooks / OpenTelemetry,Codex 的 `notify` / OpenTelemetry。
  需要改用户配置,等哪一边的格式真的断过一次再考虑(这些接口的具体字段尚未核对)。

## 分期

1. 台账与扫描(含子代理记录,保证总量不少算)、Claude 适配器、面板的用量页和会话页(含筛选)、用量条可点击。
   建议页先只上"缓存过期后续聊"和"起步上下文偏重"两条规则,加缓存倒计时。
2. Codex 适配器(含进程识别、额度显示)、技能与子代理页、其余规则、状态栏实时提示。
3. 橙灯时长并入、语音模式自身的用量记录。

## 测试

- 规则和适配器:脱敏 JSONL 片段做单测。
  - 口径:Claude 四项相加,Codex 用 `total_tokens`,同样的用量两边算出的总量和费用一致。
  - 技能:Claude 的 `Skill` 调用;Codex 的整读、部分读、同会话重复读 `SKILL.md`。
  - 子代理:Claude 的 `subagents/` 目录;Codex 带 `parent_thread_id` 的 rollout;父会话已被清理而子代理还在。
- 台账:重复扫描不重复计数、游标后文件被截短、中途崩溃后重扫。
- 界面:`shots.rs` 加 `insights_shots`,注入固定数据,四个页签各截一张。

## 未决问题

- **语音模式自己的开销**:自然语言模式默认用 `claude-opus-5-5`,每次还带 40 行屏幕内容,这部分不在 JSONL 里,
  要在 `claude.rs` 里自己记 `usage`;生成命令这种任务是否默认换成小模型。
- **超过 30 天没打开应用**:Claude 那边的记录已被清理,补不回来,面板标"无数据"。
  用 launchd 之类的系统定时任务可以消除这个缺口,目前倾向不做。
- **Codex 的保留规则**:是否自动清理会话记录,尚未核实。
- **同一项目两个窗格各跑一个 agent**:记录里没有 pid,用量条对不上具体窗格(现有限制);台账按会话 id 记,不受影响。
- **`turn_duration` 的写入条件**:本机一份记录里约 15 次提问只有 6 条,什么情况下才写没有查到。
- **没跑 agent 时面板是否可用**:倾向可用,显示全部项目的汇总。
- **有建议时是否加角标**:倾向不加,避免常驻提醒变成噪音。
- **`⌘I` 是否合适**:在很多编辑器里是"斜体"。
- **Codex `spawn_agent` 子代理的记录形态**:工具已在清单里,本机还没有实际调用,`source` / `thread_source` 的取值待有记录后核对。
- **`codex-auto-review` 等内部模型的单价**:官方价目里可能没有,按规则显示"—",但 guardian 次数多时费用会被低估。
- **Claude 技能的加载体积**:`Skill` 调用后技能正文以什么形式进入上下文、能否直接量出字节数,还没核对;手敲 `/技能名` 是否也走 `Skill` 工具同样待查。
- **原型与文档的出入**:原型用量页的总量是独立生成的示例数字,没有把子代理那部分加进去;文档以"总量包含子代理"为准。

## 第 1 期实现记录(2026-11-22)

与上文设计的出入,以代码为准:

- **一轮的定义**:从用户发出的一句话(不是工具结果、不是 isMeta)到下一句为止;本地命令没有请求,不成行。
  同一响应会拆成多条 assistant 记录(`message.id` 相同、`usage` 重复),按 id 去重。
- **游标停在最后一轮的开头**,不是文件末尾:最后一轮可能没结束,每次扫描都重新解析、再写一行,读台账时按键保留最后一行。
  文件大小和修改时间都没变就跳过(比"修改时间晚于上次扫描"更稳)。
- **台账多了几列**:`requests`、`first_context`、`peak_context`、`cache_rebuild`(隔了超过 TTL 才发、且写缓存多于读缓存的请求的缓存写量),
  缓存写拆成 5 分钟 / 1 小时两列。
- **「起步上下文偏重」改成和自己比**:Claude Code 自己的起步量本机就有 ~45K,按固定阈值几乎每个项目都会命中。
  现在只提示首条请求中位数 ≥ 你所有项目中位数 1.3 倍(且 ≥ 40K)的项目。
- **单价**:对照 Claude API 技能里的官方价目(缓存于 2026-11-03)。Haiku 5.5 的缓存读价没有单列,按通用的 0.1× 输入;
  它超过 100K 提示的高档价按轮记账分不出来,按 100K 以内计。Codex 模型未内置单价。
- **本机实测**:763 个文件(1.3 GB)首次全量扫描 1.8 秒,1493 轮,其中子代理 668 段。
- **未做**(留到后续):悬停提示里的"本会话累计 / 上一轮思考与工具耗时"、状态栏实时提示、Codex、技能与子代理页、橙灯时长。

## 修订记录

2026-11-22,讨论原型时定下的改动:

1. **用量口径**:去掉"等效输入 token"(按缓存读 0.1×、输出 5× 等折算),改为「Token 总量」(原始数量相加)+「折合费用」(按单价)两个口径。
   面板各处的"等效"列都换成 Token、费用两列;柱状图加 Token / 费用切换;建议的节省量改用金额。
2. **模型筛选改为两级**:厂家 → 模型,可整厂家勾选,也可单选或跨厂家多选;模型表同步按厂家分组。
3. **新增「技能与子代理」页签**:统计技能触发次数和加载体积、子代理启动次数,以及各自用到的模型和 token / 费用。
4. **对照本机 Codex 记录核对**:Codex 同样有技能(读 `SKILL.md`)和子代理(带 `parent_thread_id` 的独立 rollout),
   之前写的"Codex 没有技能和子代理信息"不对;并补上了 Codex token 字段的包含关系。
