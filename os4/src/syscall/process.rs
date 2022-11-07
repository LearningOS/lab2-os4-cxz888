use crate::{
    config::MAX_SYSCALL_NUM,
    mm::{address::VirtAddr, page_table::PageTable},
    task::{self, TaskStatus},
    timer::{self, MICRO_PER_SEC},
};

pub fn sys_exit(exit_code: i32) -> ! {
    log::info!("[kernel] Application exited with code {}", exit_code);
    task::exit_current_and_run_next();
    unreachable!();
}

/// APP 将 CPU 控制权交给 OS，由 OS 决定下一步。
///
/// 总是返回 0.
///
/// syscall ID: 124
pub fn sys_yield() -> isize {
    task::suspend_current_and_run_next();
    0
}

#[repr(C)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// `_tz` 在我们的实现中忽略
///
/// syscall ID: 169
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let page_table = PageTable::from_satp(task::current_user_satp());
    let ts_va = VirtAddr(ts as usize);
    let ts_mut = page_table
        .translate(ts_va.floor())
        .unwrap()
        .ppn()
        .as_mut_at::<TimeVal>(ts_va.page_offset());
    let us = timer::get_time_us();
    ts_mut.sec = us / MICRO_PER_SEC;
    ts_mut.usec = us % MICRO_PER_SEC;
    // unsafe {
    //     (*ts).sec = us / MICRO_PER_SEC;
    //     (*ts).usec = us % MICRO_PER_SEC;
    // }
    0
}

pub struct TaskInfo {
    status: TaskStatus,
    syscall_times: [u32; MAX_SYSCALL_NUM],
    time: usize,
}

/// 查询任务信息。syscall_id = 410
///
/// 成功返回 0，错误返回 -1
///
/// NOTE: 但目前似乎没有错误的情况？
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let page_table = PageTable::from_satp(task::current_user_satp());
    let ti_va = VirtAddr(ti as usize);
    let ti_mut = page_table
        .translate(ti_va.floor())
        .unwrap()
        .ppn()
        .as_mut_at::<TaskInfo>(ti_va.page_offset());
    ti_mut.status = TaskStatus::Running;
    task::set_syscall_times(&mut ti_mut.syscall_times);
    let start_time = task::start_time();
    let now = timer::get_time_ms();
    ti_mut.time = now - start_time;
    0
}
