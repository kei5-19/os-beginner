use uefi::table::boot::MemoryType;

pub const UEFI_PAGE_SIZE: usize = 4096;

/// メモリ領域が使用可能かどうかを、メモリタイプから得る。
pub fn is_available(memory_type: MemoryType) -> bool {
    matches!(
        memory_type,
        MemoryType::BOOT_SERVICES_CODE | MemoryType::BOOT_SERVICES_DATA | MemoryType::CONVENTIONAL
    )
}
