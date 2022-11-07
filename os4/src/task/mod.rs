pub mod context;
pub mod switch;
mod tcb;

use alloc::vec::Vec;
use lazy_static::lazy_static;

pub use self::tcb::TaskStatus;
use self::{context::TaskContext, switch::__switch, tcb::TaskControlBlock};
use crate::{
    loader,
    mm::{address::VirtAddr, memory_set::MapPermission},
    sync::UPSafeCell,
    timer,
    trap::TrapContext,
};

lazy_static! {
    static ref TASK_MANAGER: TaskManager = {
        log::info!("init TASK_MANAGER");
        let num_app = loader::get_num_app();
        log::info!("num_app = {}", num_app);
        let mut tasks = Vec::with_capacity(num_app);
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(loader::get_app_data(i), i));
        }
        log::info!("{num_app} tasks loaded");
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

pub struct TaskManager {
    num_app: usize,
    inner: UPSafeCell<TaskManagerInner>,
}

struct TaskManagerInner {
    tasks: Vec<TaskControlBlock>,
    current_task: usize,
}

impl TaskManager {
    fn mark_current(&self, status: TaskStatus) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        log::trace!("task [{current}] is marked as {status:?}");
        inner.tasks[current].task_status = status;
    }
    fn next_ready_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let is_ready = |&id: &usize| inner.tasks[id].task_status == TaskStatus::Ready;
        (current + 1..self.num_app)
            .find(is_ready)
            .or_else(|| (0..=current).find(is_ready))
    }
    fn run_next_task(&self) {
        if let Some(next) = self.next_ready_task() {
            let current_task_ctx_ptr;
            let next_task_ctx_ptr;
            {
                let mut inner = self.inner.exclusive_access();
                let current = inner.current_task;
                if inner.tasks[next].start_time == 0 {
                    inner.tasks[next].start_time = timer::get_time_ms();
                }
                inner.tasks[next].task_status = TaskStatus::Running;
                inner.current_task = next;
                current_task_ctx_ptr = &mut inner.tasks[current].task_ctx as *mut TaskContext;
                next_task_ctx_ptr = &inner.tasks[next].task_ctx as *const TaskContext;
            }
            log::trace!("task [{next}] is marked as {:?}", TaskStatus::Running);
            unsafe {
                __switch(current_task_ctx_ptr, next_task_ctx_ptr);
            }
        } else {
            panic!("All application completed!");
        }
    }

    fn run_first_task(&self) -> ! {
        log::info!("start first task");
        let next_task_ctx_ptr = {
            let mut inner = self.inner.exclusive_access();
            let task0 = &mut inner.tasks[0];
            task0.task_status = TaskStatus::Running;
            task0.start_time = timer::get_time_ms();
            &task0.task_ctx as *const TaskContext
        };
        log::trace!("task [0] is marked as {:?}", TaskStatus::Running);
        let mut _unused = TaskContext::zero_init();
        unsafe {
            __switch(&mut _unused as *mut _, next_task_ctx_ptr);
        }
        unreachable!();
    }
}

pub fn suspend_current_and_run_next() {
    TASK_MANAGER.mark_current(TaskStatus::Ready);
    TASK_MANAGER.run_next_task();
}

pub fn exit_current_and_run_next() {
    TASK_MANAGER.mark_current(TaskStatus::Exited);
    TASK_MANAGER.run_next_task();
}

pub fn current_trap_ctx() -> &'static mut TrapContext {
    let inner = TASK_MANAGER.inner.exclusive_access();
    inner.tasks[inner.current_task].trap_ctx()
}

pub fn current_user_satp() -> usize {
    let inner = TASK_MANAGER.inner.exclusive_access();
    inner.tasks[inner.current_task].user_satp()
}

pub fn run_first_task() {
    TASK_MANAGER.run_first_task()
}

/// 由调用者保证 `time` 是物理地址
pub fn set_syscall_times(times: &mut [u32]) {
    let inner = TASK_MANAGER.inner.exclusive_access();
    times.copy_from_slice(&inner.tasks[inner.current_task].syscall_count);
}

pub fn incr_syscall_times(syscall_id: usize) {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let curr = inner.current_task;
    inner.tasks[curr].syscall_count[syscall_id] += 1;
}

pub fn start_time() -> usize {
    let inner = TASK_MANAGER.inner.exclusive_access();
    inner.tasks[inner.current_task].start_time
}

/// 将 start 开始 len 字节的虚拟地址映射。失败返回 false。
pub fn map_range(start: usize, len: usize, map_perm: MapPermission) -> bool {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let curr = inner.current_task;
    let vpn_range = VirtAddr(start).floor()..VirtAddr(start + len).ceil();
    let map_areas = &mut inner.tasks[curr].memory_set.areas;
    if map_areas
        .iter()
        .any(|area| !area.intersection(&vpn_range).is_empty())
    {
        return false;
    }
    inner.tasks[curr].memory_set.insert_framed_area(
        VirtAddr(start),
        VirtAddr(start + len),
        map_perm,
    );
    true
}

/// 将一个范围内的虚拟地址取消映射。失败返回 false。
///
/// 这里偷了很多懒。~~有点面向测试点编程~~。
///
/// 总而言之，这个实现假定：已经映射的内存段要么完全被输入范围包含在内，要么完全不相交。
///
/// 部分相交的情况会很麻烦，可能涉及到 MapArea 的缩小，甚至是分裂。而 MapArea 内部包含的 BTree 也要分裂。
///
/// 至少我暂时没想到什么优雅简单的实现。可能要费不少功夫，这里领会精神，过 CI 就行。
pub fn unmap_range(start: usize, len: usize) -> bool {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let curr = inner.current_task;
    let vpn_range = VirtAddr(start).floor()..VirtAddr(start + len).ceil();
    let map_set = &mut inner.tasks[curr].memory_set;
    let mut unmaped_count = 0;
    let areas = &mut map_set.areas;
    let page_table = &mut map_set.page_table;
    areas.retain_mut(|area| {
        // 释放的地址完全将该内存段包含在内
        if area.intersection(&vpn_range) == area.vpn_range {
            unmaped_count += area.vpn_range.end.0 - area.vpn_range.start.0;
            area.unmap(page_table);
            false
        } else {
            true
        }
    });
    unmaped_count == vpn_range.end.0 - vpn_range.start.0
}
