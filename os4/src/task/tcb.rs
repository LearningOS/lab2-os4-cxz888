use crate::{
    config::{self, MAX_SYSCALL_NUM, TRAP_CONTEXT},
    mm::{
        address::{PhysPageNum, VirtAddr},
        memory_set::{MapPermission, MemorySet, KERNEL_SPACE},
    },
    trap::{self, TrapContext},
};

use super::context::TaskContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    UnInit,
    Ready,
    Running,
    Exited,
}

pub struct TaskControlBlock {
    pub task_ctx: TaskContext,
    pub task_status: TaskStatus,
    pub memory_set: MemorySet,
    pub trap_ctx_ppn: PhysPageNum,
    /// 统计应用数据的大小，包括用户栈
    pub base_size: usize,
    pub syscall_count: [u32; MAX_SYSCALL_NUM],
    pub start_time: usize,
}

impl TaskControlBlock {
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        log::info!("init task: {app_id}");
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_ctx_ppn = memory_set
            .translate(VirtAddr(TRAP_CONTEXT).floor())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        let (kernel_stack_top, kernel_stack_bottom) = config::kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            VirtAddr(kernel_stack_top),
            VirtAddr(kernel_stack_bottom),
            MapPermission::R | MapPermission::W,
        );
        let task_control_block = Self {
            task_status,
            task_ctx: TaskContext::goto_trap_return(kernel_stack_bottom),
            memory_set,
            trap_ctx_ppn,
            base_size: user_sp,
            syscall_count: [0; MAX_SYSCALL_NUM],
            start_time: 0,
        };
        let trap_ctx = task_control_block.trap_ctx();
        *trap_ctx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().satp(),
            kernel_stack_bottom,
            trap::trap_handler as usize,
        );

        task_control_block
    }
    pub fn trap_ctx(&self) -> &'static mut TrapContext {
        self.trap_ctx_ppn.as_mut()
    }
    pub fn user_satp(&self) -> usize {
        self.memory_set.satp()
    }
}
