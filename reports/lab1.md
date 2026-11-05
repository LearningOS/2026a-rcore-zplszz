# Lab 1: 多道程序与分时多任务

## 功能总结

实现 trace 的字节读写与逐任务系统调用计数。分发前记账，因此查询 trace 时包含本次调用。计数表在堆上分配，避免扩大启动栈上的任务数组；无效请求或越界编号返回 -1。本章没有分页隔离，直接访问地址，volatile 只保证访问不被省略，不提供安全检查。

## 问答

### 特权隔离实验

使用 RustSBI 0.4.1、QEMU 8.2.2。官方三个 bad 程序分别触发非法地址写入、U 态执行 sret、U 态访问 sstatus。前者输出 PageFault 与地址 0，后两者输出 IllegalInstruction；内核终止相应任务后继续执行其他任务。异常并未让用户程序获得 S 态权限。

### 陷阱入口与返回

1. 进入 __restore 时，sp 指向内核栈中的 TrapContext。它用于首次启动任务，也用于处理陷阱后恢复被中断任务。
2. 从偏移 32、33、2 个机器字的位置读出 sstatus、sepc、用户 sp，分别恢复特权状态、返回指令地址、用户栈指针。用户 sp 先放入 sscratch，以便最后交换。
3. x2 是 sp，恢复过程中仍需用作内核上下文指针，最后单独恢复。x4 是 tp，本章不使用用户线程局部存储，汇编没有保存、恢复它；不能推广为通用 ABI 都允许省略 tp。
4. __restore 最后的 csrrw sp, sscratch, sp 之后，sp 为用户栈，sscratch 为已弹出陷阱帧的内核栈顶，供下一次陷阱使用。
5. 真正改变特权级的是 sret。初始化或保存的 sstatus.SPP 为 U，sret 依据该位返回 U 态，并从 sepc 继续执行。
6. __alltraps 开头相同的交换之后，sp 为内核栈顶，sscratch 为用户栈指针；随后分配陷阱帧并保存现场。
7. 本章系统调用由 U 态 ecall 触发异常，硬件完成 U 到 S 的切换并跳到 stvec；时钟中断、非法指令、访存异常也可以触发这一转换，并非入口汇编主动提高权限。

## 实际验证

课程指定 nightly-2024-05-02、cargo-binutils 0.4.0、QEMU 8.2.2。运行 make -C ci-user test CHAPTER=3，退出码 0，官方检查器 7/7、lab1 文件检查通过。初始化时曾因计数数组过大破坏启动栈，改为 Vec 堆分配后重测通过。

## 参考资料

本仓库章节框架、LearningOS rCore-Tutorial-Guide 对应章节、官方 rCore-Tutorial-Test 与 rCore-Tutorial-Checker。
