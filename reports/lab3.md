# 实验三：进程管理与 stride 调度

## 功能总结

迁移用户地址检查、跨页时间写回和匿名映射；实现直接从 ELF 创建子进程的 spawn，建立父子关系并加入就绪队列；以 stride 最小的任务作为下一个运行任务，默认优先级为 16，支持合法优先级更新。进程 exec 后同步重置堆起点及 program_brk，避免沿用旧地址空间的堆状态。

## stride 溢出分析

8 位无符号 stride 中，p2 的 250 加 10 后回绕成 4。普通无符号比较会再次选择 p2，而非理论上的 p1，这是计数器回绕破坏数值顺序造成的。

初始 stride 都为零。若每次只增加最小 stride，且每个 pass 不超过 BigStride/2，那么新值最多超过旧最小值 BigStride/2，其他任务仍位于这个窗口内；因此无溢出、没有额外插入新任务时，最大最小差值保持在该界限内。中途插入 stride 为零的新任务会打破前提，实际系统可用当前最小 stride 初始化新任务。

环形计数器比较可使用 wrapping_sub 判断距离是否跨越半环。BinaryHeap 默认取最大值，若需要弹出逻辑最小 stride，还必须反转 Ordering，或给正常比较器包一层 Reverse。假设 BigStride=u64::MAX，差值严格小于半环且值不相等，可写为：


```rust
impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let d = self.0.wrapping_sub(other.0);
        Some(if d > u64::MAX / 2 {
            Ordering::Greater // self logically smaller; reverse for max-heap
        } else {
            Ordering::Less
        })
    }
}
```

## 实现选择

当前实现使用 u128 stride 和 2^48 的 BigStride，线性扫描就绪队列。pass 至少为 1，避免极大优先级整数除法得到零导致调度停滞；扩大累加器后，正常课程测试及实际可运行时长不会接近溢出。每次调度前按 priority 计算 pass，优先级修改立即作用于后续调度。

## 测试

执行官方 Checker：make -C ci-user test CHAPTER=5。功能测试 15/15，包括 spawn、waitpid、get_time、mmap/munmap、前章回归和优先级。一次 stride 观测中各子进程 runtimes/priority 在 3780800 至 3987920 之间，最大/最小约 1.055，小于 1.5。

## 参考资料

本仓库 ch5 框架、LearningOS rCore-Tutorial-Guide chapter5/4exercise、官方 rCore-Tutorial-Test 与 rCore-Tutorial-Checker。
