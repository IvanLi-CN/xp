# Implementation

## Current state

- bootstrap、join、PHC 同步和 leader 版本跟随已实现。
- 管理员凭据只以 PHC 形式落盘；join metadata 保存同一集群 PHC。
- 当前任务将默认 Argon2 参数迁到 4 MiB 档，并轮换生产集群 Token。

## Remaining verification

- 三种部署形态必须通过低内存 PHC 升级和回滚测试。
- 生产四节点必须验证 PHC 指纹一致。
