# 把已存在于 DesiredState 的节点重新纳入 Raft

- Status: accepted

## 背景

节点可以处于两种彼此独立的状态：它是 DesiredState Node（集群打算管理的节点），以及它是
Raft Voter（持有投票身份）。稳态要求每个 voter 对应一个 DesiredState Node，但反过来不
成立——一个 DesiredState Node 可以在没有 Raft 身份的情况下存在。

单节点 quorum 恢复会把 Raft 收缩为幸存的那一个 voter。如果其余节点只是暂时不可达、机器仍在
运行，恢复完成后集群就停在「一个 voter、多个 DesiredState Node」的形状。该形状违反成员
不变量：

- `missing_desired_members` 是 DesiredState 与 Raft 的差集，此时恒非空；
- 该审计非空会同时拒绝 fresh join 与 remove-node；
- 因此没有任何 CLI 能再把集群收敛回去。

现场的形态是：6 个 DesiredState Node、1 个 voter，其中 3 台仍在正常服务用户流量，2 台永久失联。

## 决定

新增显式的 rejoin 路径，而不是放宽 fresh join 的语义。

- leader 侧 `xp-ops xp rejoin-node --node-id <id>` 只做 fresh join 缺的那一步：确认目标是
  Absent DesiredState Node，记录既有的 `Restore` 生命周期，并签发绑定该身份的 join token。
  `Restore` 已经拥有 learner 注册、追平与提升，因此不需要新的成员变更实现。
- 节点侧仍执行标准 `xp join`，复用已有的 CSR、证书签发、learner 注册与提升流程。
- 目标的 endpoint 与端口完全不变，因此数据面不中断。
- 永久失联的节点不参与 rejoin，由删除路径清理。

删除「DesiredState 有、Raft 无」的节点沿用 `evict-unreachable-voter`，为其增加显式的
「已确认缺席」输入。该模式容忍整个缺席集合（它本身就是删除工作队列），但仍拒绝孤儿 voter、
重复身份与异常 learner。capability 探测仍只针对当前 voter，因此删除不等待缺席节点应答。

顺序不强制：rejoin 与删除互不依赖，文档建议先 rejoin 再删除，使 capability 探测覆盖更多真实 voter。

## 考虑过的备选

- **直接放宽 fresh-join 准入，允许 DesiredState 已存在的目标。** 拒绝。fresh join 与 rejoin 是
  不同语义：前者建立新身份与新 endpoint，后者恢复既有身份与既有 endpoint。合并后无法区分
  「重新纳入」与「重复注册」，也无法在准入时证明目标确实是 Absent。
- **恢复时无条件 prune 全部非成员 DesiredState。** 拒绝。它把仍在服务流量的节点一并清掉，用控制面一致性换取数据面中断。
- **为删除新写一条命令。** 拒绝。删除节点应只有一个入口；`evict-unreachable-voter` 已经是
  「删除确认永久失联的节点」的入口，扩展它比新增命令更能防止语义漂移。
- **保留 DesiredState 记录，改为让不变量忽略它们。** 拒绝。那会把一个恢复期例外扩散到每一次生命周期写入，削弱 delete、restore、eviction 依赖的安全契约。

## 后果

- 「DesiredState Node」与「Raft Voter」的区分进入领域词汇；不变量继续按差集判定，不做豁免。
- rejoin 不改变节点身份、证书或 endpoint，因此可以在用户流量不中断的情况下执行。
- fresh join 仍然只接受全新身份；ADR 0010 关于 `unexpected_learners` 的豁免范围不变。
- 删除缺席节点需要操作者显式声明它已永久失联，与现有 eviction 的确认风格一致。
- 如果 rejoin 在记录 `Prepared` 后中断，持久操作会按既有 resume 契约恢复或进入 `Blocked`，不会伪造成功。
