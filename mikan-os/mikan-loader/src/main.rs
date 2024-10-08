#![no_std]
#![no_main]

mod chars;
mod elf;
mod graphics;

use crate::chars::*;
use crate::elf::Elf64Ehdr;
use core::{
    arch::asm,
    ffi::c_void,
    fmt::Write,
    mem::{size_of, transmute},
    ptr::{copy_nonoverlapping, write_bytes},
    slice,
};
use elf::{Elf64Phdr, ProgramType};
use graphics::FrameBufferConfig;
use graphics::GraphicsInfo;
use log::{error, info};
use uefi::{
    data_types::Identify,
    prelude::*,
    proto::{
        console::gop::{GraphicsOutput, PixelFormat},
        loaded_image::LoadedImage,
        media::{
            block::BlockIO,
            file::{Directory, File, FileAttribute, FileHandle, FileInfo, FileMode, RegularFile},
            fs::SimpleFileSystem,
        },
    },
    table::{
        boot::{
            AllocateType, MemoryDescriptor, MemoryMap, MemoryType, OpenProtocolAttributes,
            OpenProtocolParams, ScopedProtocol, SearchType,
        },
        runtime::Time,
    },
    CStr16, Error, Guid, Result,
};

/// メモリマップを渡されたファイルに保存する。
fn save_memory_map(
    system_table: &mut SystemTable<Boot>,
    map: &MemoryMap,
    file: &mut RegularFile,
) -> Status {
    // ファイルに書き込むヘッダ
    let header = b"Index, Type, Type(name), PhysicalStart, NumberOfPages, Attribute\n";
    let _ = file.write(header);

    // フォーマットしてコンソールに出力
    // フォーマットするには、それ用のバッファを作っておかなければならない
    let mut buf16 = [0u16; 128];
    let mut str16_buf = Str16Buf::new(&mut buf16);

    let _ = write!(
        str16_buf,
        "map->buffer = {:08x}, map->map_size = {:08x}\r\n",
        map as *const MemoryMap as usize,
        size_of::<MemoryDescriptor>() * map.entries().count()
    );
    let _ = system_table.stdout().output_string(str16_buf.into_cstr16());

    let mut i = 0;
    let mut entries = map.entries();
    // メモリマップの各エントリを書き出す
    while let Some(desc) = entries.next() {
        // フォーマットするためのバッファ
        let mut buf8 = [0u8; 256];
        let mut str8_buf = Str8Buf::new(&mut buf8);

        let _ = write!(
            str8_buf,
            "{}, {}, {}, {:08x}, {}, {}\n",
            i,
            desc.ty.0,
            get_memory_type_unicode(desc.ty),
            desc.phys_start,
            desc.page_count,
            desc.att.bits()
        );

        let _ = file.write(str8_buf.get());
        i += 1;
    }

    Status::SUCCESS
}

/// メモリマップを取得してそれを [MemoryMap] として返す。
fn get_memory_map<'a>(services: &BootServices, map: &'a mut [u8]) -> uefi::Result<MemoryMap<'a>> {
    if map.len() == 0 {
        return Err(uefi::Error::new(Status::BUFFER_TOO_SMALL, ()));
    }

    Ok(services.memory_map(map)?)
}

/// メモリのタイプ情報から、意味を表す 16 bit 文字列を返す。
fn get_memory_type_unicode(r#type: MemoryType) -> &'static CStr16 {
    match r#type {
        MemoryType::RESERVED => cstr16!("EfiReservedMemoryType"),
        MemoryType::LOADER_CODE => cstr16!("EfiLoaderCode"),
        MemoryType::LOADER_DATA => cstr16!("EfiLoaderData"),
        MemoryType::BOOT_SERVICES_CODE => cstr16!("EfiBootServicesCode"),
        MemoryType::BOOT_SERVICES_DATA => cstr16!("EfiBootServicesData"),
        MemoryType::RUNTIME_SERVICES_CODE => cstr16!("EfiRuntimeServicesCode"),
        MemoryType::RUNTIME_SERVICES_DATA => cstr16!("EfiRuntimeServicesData"),
        MemoryType::CONVENTIONAL => cstr16!("EfiConventionalMemory"),
        MemoryType::UNUSABLE => cstr16!("EfiUnusableMemory"),
        MemoryType::ACPI_RECLAIM => cstr16!("EfiACPIReclaimMemory"),
        MemoryType::ACPI_NON_VOLATILE => cstr16!("EfiAcpiMemoryNVS"),
        MemoryType::MMIO => cstr16!("EfiMemoryMappedIO"),
        MemoryType::MMIO_PORT_SPACE => cstr16!("EfiMemoryMappedIOPortSpace"),
        MemoryType::PAL_CODE => cstr16!("EfiPalCode"),
        MemoryType::PERSISTENT_MEMORY => cstr16!("EfiPersistentMemory"),
        _ => cstr16!("InvalidMemoryType"),
    }
}

/// ルートディレクトリを操作するオブジェクト（[Directory]）を返す。
/// 失敗した場合は [uefi::Error] が返る。
fn open_root_dir(services: &BootServices, image_handle: Handle) -> uefi::Result<Directory> {
    // 恐らく、ロードしているデバイスを操作するオブジェクトの取得
    let loaded_image = match match unsafe {
        services.open_protocol::<LoadedImage>(
            OpenProtocolParams {
                handle: image_handle,
                agent: image_handle,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )?
    }
    .get_mut()
    {
        None => return Err(uefi::Error::new(Status::ABORTED, ())),
        Some(proto) => proto,
    }
    .device()
    {
        None => return Err(uefi::Error::new(Status::ABORTED, ())),
        Some(handle) => handle,
    };

    // デバイスのファイルシステム操作用オブジェクトの取得
    let binding = unsafe {
        services.open_protocol::<SimpleFileSystem>(
            OpenProtocolParams {
                handle: loaded_image,
                agent: image_handle,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )?
    };
    let fs = match binding.get_mut() {
        None => return Err(uefi::Error::new(Status::ABORTED, ())),
        Some(proto) => proto,
    };

    // ルートディレクトリを開いて返す
    Ok(fs.open_volume()?)
}

/// 画面出力情報を取得する。
fn get_gop_info(
    image_handle: Handle,
    system_table: &mut SystemTable<Boot>,
) -> uefi::Result<GraphicsInfo> {
    // GOP を操作するためのオブジェクト
    let gop_handles = system_table
        .boot_services()
        .locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?;

    // GOP を取得
    let mut gop = unsafe {
        system_table
            .boot_services()
            .open_protocol::<GraphicsOutput>(
                OpenProtocolParams {
                    handle: (*gop_handles)[0],
                    agent: image_handle,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )?
    };

    let pixel_info = match gop.get() {
        None => return Err(uefi::Error::new(Status::ABORTED, ())),
        Some(gop) => gop.current_mode_info(),
    };

    Ok(GraphicsInfo {
        pixel_info,
        frame_buffer_base: gop.frame_buffer().as_mut_ptr() as usize,
        _frame_buffer_size: gop.frame_buffer().size(),
    })
}

/// ピクセルのデータ形式情報を文字列にする。
fn get_pixel_format_unicode(fmt: PixelFormat) -> &'static CStr16 {
    match fmt {
        PixelFormat::Rgb => cstr16!("PixelRedGreenBlueReserved8bitPerColor"),
        PixelFormat::Bgr => cstr16!("PixelBlueGreenRedReserved8BitPerColor"),
        PixelFormat::Bitmask => cstr16!("PixelBitMask"),
        PixelFormat::BltOnly => cstr16!("PixelBltOnly"),
    }
}

/// ELF ファイルを展開するときの、最下位アドレスと最上位アドレスを返す。
/// 戻り値は (最下位, 最上位)。
fn calc_load_address_range(phdrs: &[Elf64Phdr]) -> (usize, usize) {
    let mut first = usize::MAX;
    let mut last = usize::MIN;
    for phdr in phdrs {
        if phdr.r#type != ProgramType::Load as u32 {
            continue;
        }
        first = usize::min(first, phdr.vaddr);
        last = usize::max(last, phdr.vaddr + phdr.memsz as usize);
    }
    (first, last)
}

/// メモリに展開された ELF ファイルのヘッダ情報を元に、指定されたアドレスへ命令等を配置する。
/// ただし、この関数を呼ぶ前に必要領域のページ割り当てを行っておくこと。
fn copy_load_segments(src_base: usize, phdrs: &[Elf64Phdr]) {
    for phdr in phdrs {
        if phdr.r#type != ProgramType::Load as u32 {
            continue;
        }

        // 指定アドレスへのコピー
        let segm_in_file = src_base + phdr.offset as usize;
        unsafe {
            copy_nonoverlapping(
                segm_in_file as *const u8,
                phdr.vaddr as *mut u8,
                phdr.filesz as usize,
            );
        }

        // 残りの部分の 0 埋め
        let remein_base = (phdr.vaddr + phdr.filesz as usize) as *mut u8;
        let remain_bytes = (phdr.memsz - phdr.filesz) as usize;
        unsafe {
            write_bytes(remein_base, 0, remain_bytes);
        }
    }
}

fn read_file(
    system_table: &mut SystemTable<Boot>,
    mut file: FileHandle,
) -> Result<&'static mut [u8]> {
    // ファイル情報を取得
    const FILE_INFO_SIZE: usize = size_of::<u64>() * 2
        + size_of::<Time>() * 3
        + size_of::<FileAttribute>()
        // （恐らく）ここまでがファイル名以外の情報のためのサイズ
        // ここからはファイル名のための情報
        // 文字スライスが null 終端されていない代わりに、長さの情報を持っている（と思われる）
        + size_of::<usize>()
        // FAT のファイル名は 11 文字と制限されているから、11文字分あれば十分
        + size_of::<u16>() * 11;

    // ファイル情報保持のためのバッファ
    let mut file_info_buffer = [0u8; FILE_INFO_SIZE];
    let file_info = match file.get_info::<FileInfo>(&mut file_info_buffer) {
        Err(e) => {
            error!("Failed to get file info: {}", e);
            halt();
        }
        Ok(info) => info,
    };

    let file_size = file_info.file_size();

    // ファイル全体を扱うオブジェクトから、レギュラーファイル用オブジェクトに変換
    let mut file = match file.into_regular_file() {
        None => {
            error!("Opend file isn't a regular file");
            return Err(Error::new(Status::UNSUPPORTED, ()));
        }
        Some(file) => file,
    };

    // ファイル展開用のプールを取得
    let buffer_addr = system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, file_size as usize)?;
    let buffer = unsafe { slice::from_raw_parts_mut(buffer_addr, file_size as usize) };

    // ファイルを展開
    file.read(buffer)?;

    Ok(buffer)
}

fn open_block_io_protocol_for_loaded_image(
    system_table: &SystemTable<Boot>,
    image_handle: Handle,
) -> Result<ScopedProtocol<'_, BlockIO>> {
    unsafe {
        let loaded_image = match system_table
            .boot_services()
            .open_protocol::<LoadedImage>(
                OpenProtocolParams {
                    handle: image_handle,
                    agent: image_handle,
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )?
            .device()
        {
            Some(image) => image,
            None => return Err(uefi::Error::new(Status::ABORTED, ())),
        };

        system_table.boot_services().open_protocol::<BlockIO>(
            OpenProtocolParams {
                handle: loaded_image,
                agent: image_handle,
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
}

fn read_block(
    system_table: &SystemTable<Boot>,
    block_io: &mut BlockIO,
    media_id: u32,
    read_bytes: u64,
) -> Result<&'static mut [u8]> {
    let read_bytes = read_bytes as usize;

    let buffer_addr = system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, read_bytes)?;

    let buffer = unsafe { slice::from_raw_parts_mut(buffer_addr, read_bytes) };
    block_io.read_blocks(media_id, 0, buffer)?;
    Ok(buffer)
}

#[entry]
fn efi_main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // 恐らく log を使えるようにしているのではないか
    match uefi_services::init(&mut system_table) {
        Err(_) => {
            system_table
                .stderr()
                .output_string(cstr16!("Failed to initialize\r\n"))
                // 流石にこれの失敗はもう無視するしかない
                .unwrap_or_default();
            halt()
        }
        Ok(_) => (),
    }

    match system_table
        .stdout()
        .output_string(cstr16!("Hello, World!\r\n"))
    {
        Err(e) => {
            error!("Failed to print: {}", e);
            halt();
        }
        Ok(_) => (),
    }

    // メモリマップの取得
    let mut memmap_buf = [0u8; 4096 * 4];
    let memmap = match get_memory_map(system_table.boot_services(), &mut memmap_buf) {
        Err(e) => {
            error!("Failed to get memmap: {}", e);
            halt();
        }
        Ok(map) => map,
    };

    // ルートディレクトリ操作用のオブジェクトの取得
    let mut root_dir = match open_root_dir(system_table.boot_services(), image_handle) {
        Err(e) => {
            error!("Failed to open root dir: {}", e);
            halt();
        }
        Ok(dir) => dir,
    };

    // メモリマップ保存用ファイルを操作するオブジェクトの取得
    let mut memmap_file = match root_dir.open(
        cstr16!("\\memmap"),
        FileMode::CreateReadWrite,
        FileAttribute::empty(),
    ) {
        Err(e) => {
            error!("Failed to open a file: {}", e);
            halt();
        }
        Ok(file) => match file.into_regular_file() {
            None => {
                // これは流石に起こらないと思うが、一応
                error!("Opend file isn't a regular file");
                halt();
            }
            Some(file) => file,
        },
    };

    // メモリマップを上で取得したファイルに保存する
    let _ = save_memory_map(&mut system_table, &memmap, &mut memmap_file);
    memmap_file.close();

    // 画面情報の取得
    let graphics_info = match get_gop_info(image_handle, &mut system_table) {
        Err(e) => {
            error!("Failed to get gop info: {}", e);
            halt();
        }
        Ok(info) => info,
    };

    // 画面情報の表示
    let mut buf16 = [0u16; 128];
    let mut str16_buf = Str16Buf::new(&mut buf16);
    match write!(
        str16_buf,
        "Resolution: {}x{}, Pixel Format: {}, {} pixels/line\r\n",
        graphics_info.pixel_info.resolution().0,
        graphics_info.pixel_info.resolution().1,
        get_pixel_format_unicode(graphics_info.pixel_info.pixel_format()),
        graphics_info.pixel_info.stride()
    ) {
        Err(e) => {
            error!("Failed to write on the buffer: {}", e);
            halt()
        }
        Ok(_) => (),
    };
    match system_table.stdout().output_string(str16_buf.into_cstr16()) {
        Err(e) => {
            error!("Failed to print: {}", e);
            halt();
        }
        Ok(_) => (),
    };

    // `\kernel.elf` を開く
    let kernel_file =
        match root_dir.open(cstr16!("\\kernel"), FileMode::Read, FileAttribute::empty()) {
            Err(e) => {
                error!("Failed to open kernel: {}", e);
                halt();
            }
            Ok(file) => file,
        };

    let kernel_buffer = match read_file(&mut system_table, kernel_file) {
        Ok(buf) => buf,
        Err(e) => {
            error!("error: {}", e);
            halt();
        }
    };

    // kernel.elf の ELF ヘッダを取得
    let kernel_buffer_addr = kernel_buffer.as_ptr();
    let kernel_ehdr = unsafe { *(kernel_buffer_addr as *const Elf64Ehdr) };
    let phdr_addr = kernel_buffer_addr as usize + kernel_ehdr.phoff as usize;
    let kernel_phdrs =
        unsafe { slice::from_raw_parts(phdr_addr as *const Elf64Phdr, kernel_ehdr.phnum as usize) };

    // カーネルを展開する最下位・最上位ビットを得る
    let (kernel_first_addr, kernel_last_addr) = calc_load_address_range(&kernel_phdrs);

    // ページの割り当て
    // ページサイズは 4 KiB
    let num_pages = ((kernel_last_addr - kernel_first_addr + 0xfff) / 0x1000) as usize;
    match system_table.boot_services().allocate_pages(
        AllocateType::Address(kernel_first_addr as u64),
        MemoryType::LOADER_DATA,
        num_pages,
    ) {
        Err(e) => {
            error!("Failed to allocate pages: {}", e);
            halt();
        }
        Ok(_) => (),
    };

    // カーネルのロード
    copy_load_segments(kernel_buffer_addr as usize, &kernel_phdrs);

    // カーネルを読み込んだ位置を表示
    str16_buf.clear();
    match write!(
        str16_buf,
        "Kernel: 0x{:0x} - 0x{:0x}\r\n",
        kernel_first_addr, kernel_last_addr
    ) {
        Err(e) => {
            error!("Failed to write on the buffer: {}", e);
            halt();
        }
        Ok(_) => (),
    };
    match system_table.stdout().output_string(str16_buf.into_cstr16()) {
        Err(e) => {
            error!("Failed to print: {}", e);
            halt();
        }
        Ok(_) => (),
    };

    // 確保してあったカーネル一次保存用のプールを解放
    unsafe {
        match system_table
            .boot_services()
            .free_pool(kernel_buffer.as_mut_ptr() as *mut u8)
        {
            Err(e) => {
                error!("Failed to free pool: {}", e);
                halt();
            }
            Ok(_) => (),
        }
    }

    // "\fat_disk" もしくは外部接続の IO デバイスを読み込む
    let volume_image = match root_dir.open(
        cstr16!("\\fat_disk"),
        FileMode::Read,
        FileAttribute::empty(),
    ) {
        Ok(file) => match read_file(&mut system_table, file) {
            Ok(buf) => buf,
            Err(e) => {
                error!("failed to read volume file: {}", e);
                halt();
            }
        },
        Err(_) => {
            // "\fat_disk" が見つからなかった場合
            let block_io =
                match open_block_io_protocol_for_loaded_image(&system_table, image_handle) {
                    Ok(prot) => prot,
                    Err(e) => {
                        error!("failed to read blocks: {}", e);
                        halt();
                    }
                };

            let media = block_io.media();
            let volume_bytes = media.block_size() as u64 * (media.last_block() + 1);
            let volume_bytes = if volume_bytes > 32 * 1024 * 1024 {
                32 * 1024 * 1024
            } else {
                volume_bytes
            };

            info!(
                "Reading {} bytes (Present {}, BockSize {}, LastBlock {})",
                volume_bytes,
                media.is_media_present(),
                media.block_size(),
                media.last_block(),
            );

            match read_block(
                &system_table,
                block_io.get_mut().unwrap(),
                media.media_id(),
                volume_bytes,
            ) {
                Ok(buf) => buf,
                Err(e) => {
                    error!("failed to read blocks: {}", e);
                    halt();
                }
            }
        }
    };

    // UEFI のブートサービスを終了する
    let (system_table, _) = system_table.exit_boot_services(MemoryType(0));

    let frame_buffer = graphics_info.frame_buffer_base;
    let pixels_per_scan_line = graphics_info.pixel_info.stride();
    let (horizontal_resolution, vertical_resolution) = graphics_info.pixel_info.resolution();
    let pixel_format = match graphics_info.pixel_info.pixel_format() {
        PixelFormat::Rgb => graphics::PixelFormat::Rgb,
        PixelFormat::Bgr => graphics::PixelFormat::Bgr,
        _ => {
            error!(
                "Unimplemented pixel format: {:?}",
                graphics_info.pixel_info.pixel_format()
            );
            halt();
        }
    };
    let config = FrameBufferConfig {
        frame_buffer,
        pixels_per_scan_line,
        horizontal_resolution,
        vertical_resolution,
        pixel_format,
    };

    // ACPI テーブルの取得
    // cf. https://github.com/tianocore/edk2/blob/948f23417010309a5557d46195eae258f6105025/MdePkg/MdePkg.dec#L380
    const ACPI_TABLE_GUID: Guid = Guid::new(
        [0x71, 0xe8, 0x68, 0x88],
        [0xf1, 0xe4],
        [0xd3, 0x11],
        0xbc,
        0x22,
        [0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
    );
    let acpi_table = system_table
        .config_table()
        .iter()
        .find(|table| table.guid == ACPI_TABLE_GUID)
        .map(|table| table.address)
        .unwrap_or_else(|| halt());

    // カーネルの呼び出し
    // ELF ファイルの 24 byte 目から 64 bit でエントリーポイントの番地が書いてある
    let entry_point: extern "sysv64" fn(
        &FrameBufferConfig,
        &MemoryMap,
        usize,
        usize,
        *const c_void,
        *mut c_void,
    ) = unsafe { transmute(kernel_ehdr.entry) };
    entry_point(
        &config,
        &memmap,
        kernel_first_addr,
        kernel_last_addr - kernel_first_addr,
        acpi_table,
        volume_image.as_mut_ptr() as *mut c_void,
    );

    halt()
}

fn halt() -> ! {
    unsafe {
        loop {
            asm!("hlt");
        }
    }
}
