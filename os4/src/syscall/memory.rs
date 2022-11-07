use crate::{config::PAGE_SIZE, mm::memory_set::MapPermission, task};

/// 本实验仅用于申请内存。syscall id = 222。成功返回 0，错误返回 -1。
///
/// `start` 要求按页对齐。port 低三位分别表示以下属性，其它位无效且必须为 0
///
/// - `port[2]`: read.
/// - `port[1]`: write.
/// - `port[0]`: exec.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    if start % PAGE_SIZE != 0 || port & !0x7 != 0 || port & 0x7 == 0 {
        return -1;
    }
    let mut map_perm = MapPermission::U;
    if port & 0x1 != 0 {
        map_perm |= MapPermission::R;
    }
    if port & 0x2 != 0 {
        map_perm |= MapPermission::W;
    }
    if port & 0x4 != 0 {
        map_perm |= MapPermission::X;
    }
    if task::map_range(start, len, map_perm) {
        0
    } else {
        -1
    }
}

/// 取消映射。syscall id = 215。成功返回 0，错误返回 -1。
///
/// `start` 要求按页对齐。
///
/// FIXME: 注意，这里的实现是钻空子的。具体请看 `task::unmap_range` 的注释
pub fn sys_munmap(start: usize, len: usize) -> isize {
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if task::unmap_range(start, len) {
        0
    } else {
        -1
    }
}
