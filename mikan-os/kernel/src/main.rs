#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use core::{ffi::c_void, panic::PanicInfo};
use uefi::table::boot::MemoryMap;

use kernel::{
    acpi::RSDP,
    asmfunc::{self, cli, halt, sti},
    console::{self, PanicConsole},
    error::Result,
    fat, font,
    frame_buffer_config::FrameBufferConfig,
    graphics::{PixelColor, PixelWrite, Vector2D, FB_CONFIG},
    interrupt, keyboard,
    layer::{self, LAYER_MANAGER, LAYER_TASK_MAP, SCREEN},
    log,
    logger::{set_log_level, LogLevel},
    memory_manager::{GLOBAL, MEMORY_MANAGER},
    message::{Message, MessageType},
    mouse, paging, pci, printk, printkln, segment, syscall,
    task::{self, Stack},
    terminal,
    timer::{self, Timer, TIMER_MANAGER},
    window::Window,
    xhci::{self, XHC},
};

#[no_mangle]
static KERNEL_MAIN_STACK: Stack<STACK_SIZE> = Stack::new();

/// メインウィンドウの初期化を行う。
fn initialize_main_window() -> u32 {
    let mut layer_manager = LAYER_MANAGER.lock_wait();

    let main_window =
        Window::new_toplevel(160, 52, SCREEN.lock_wait().pixel_format(), "Hello Window");
    let main_window_id = layer_manager.new_layer(main_window);
    layer_manager
        .layer(main_window_id)
        .r#move(Vector2D::new(300, 100))
        .set_draggable(true);

    layer_manager.up_down(main_window_id, 2);

    main_window_id
}

/// テキストウィンドウを表示、登録し、そのレイヤー ID を返す。
fn initialize_text_window() -> u32 {
    let win_w = 160;
    let win_h = 52;

    let mut window = Window::new_toplevel(
        win_w,
        win_h,
        SCREEN.lock_wait().pixel_format(),
        "Text Box Test",
    );

    let inner_size = window.size();
    window.draw_text_box(Vector2D::new(0, 0), inner_size);

    let mut layer_manager = LAYER_MANAGER.lock_wait();
    let layer_id = layer_manager.new_layer(window);
    layer_manager
        .layer(layer_id)
        .r#move(Vector2D::new(500, 100))
        .set_draggable(true);

    layer_manager.up_down(layer_id, i32::MAX);

    layer_id
}

fn draw_text_cursor(visible: bool, index: i32, window: &mut Window) {
    let color = PixelColor::to_color(if visible { 0 } else { 0xffffff });
    let pos = Vector2D::new(4 + 8 * index, 5);
    window.fill_rectangle(pos, Vector2D::new(7, 15), &color);
}

// この呼び出しの前にスタック領域を変更するため、でかい構造体をそのまま渡せなくなる
// それを避けるために参照で渡す
#[custom_attribute::kernel_entry(KERNEL_MAIN_STACK, STACK_SIZE = 1024 * 1024)]
fn kernel_entry(
    frame_buffer_config: &'static FrameBufferConfig,
    memory_map: &'static MemoryMap,
    kernel_base: usize,
    kernel_size: usize,
    acpi_table: &RSDP,
    volume_image: *mut c_void,
) {
    FB_CONFIG.init(frame_buffer_config.clone());
    // メモリアロケータの初期化
    MEMORY_MANAGER.init(memory_map, kernel_base, kernel_size);
    GLOBAL.init(64 * 512); // 128 MiB 確保

    if let Err(err) = main(acpi_table, volume_image) {
        printkln!("{}", err);
    }
}

fn main(acpi_table: &RSDP, volume_image: *mut c_void) -> Result<()> {
    layer::init();
    console::init();

    printk!("Welcome to MikanOS!\n");

    #[cfg(not(debug_assertions))]
    set_log_level(LogLevel::Warn);

    #[cfg(debug_assertions)]
    set_log_level(LogLevel::Debug);

    segment::init();
    paging::init();
    interrupt::init();

    fat::init(volume_image);
    font::init()?;
    pci::init()?;

    let main_window_id = initialize_main_window();
    let text_window_id = initialize_text_window();

    // FIXME: 最初に登録されるレイヤーは背景ウィンドウなので、`layer_id` 1 を表示すれば
    //        必ず全て表示されるが、ハードコードは良くなさそう
    LAYER_MANAGER.lock_wait().draw_id(1);

    acpi_table.init()?;
    timer::init();

    // カーソル点滅用のタイマを追加
    let textbox_cursor_timer = 1;
    let timer_05sec = (timer::TIMER_FREQ as f64 * 0.5) as u64;
    TIMER_MANAGER
        .lock_wait()
        .add_timer(Timer::new(timer_05sec, textbox_cursor_timer, 1));
    let mut textbox_cursor_visible = false;

    syscall::init();

    task::init();
    let main_task = task::current_task();
    task::new_task()
        .init_context(terminal::task_terminal, 0, 0)
        .wake_up(-1);

    xhci::init();
    mouse::init();
    keyboard::init();

    // debug 時のみライブラリのテストを動かす。
    #[cfg(debug_assertions)]
    test_lib();

    let mut text_window_index = 0;
    loop {
        let tick = match TIMER_MANAGER.lock() {
            Some(manager) => manager.current_tick(),
            None => {
                main_task.sleep();
                continue;
            }
        };
        let active = {
            let mut layer_manager = match LAYER_MANAGER.lock() {
                Some(manager) => manager,
                None => {
                    main_task.sleep();
                    continue;
                }
            };
            let window = layer_manager.layer(main_window_id).window();
            window.write().fill_rectangle(
                Vector2D::new(20, 4),
                Vector2D::new(8 * 10, 16),
                &PixelColor::new(0xc6, 0xc6, 0xc6),
            );
            font::write_string(
                &mut *window.write(),
                Vector2D::new(20, 4),
                format!("{:010}", tick).as_str(),
                &PixelColor::new(0, 0, 0),
            );

            layer_manager.draw_id(main_window_id);
            layer_manager.get_active()
        };

        cli();
        let msg = match main_task.receive_message() {
            Some(msg) => msg,
            None => {
                main_task.sleep();
                sti();
                continue;
            }
        };
        sti();

        match msg.ty {
            MessageType::InterruptXHCI => {
                let mut xhc = XHC.lock_wait();
                while xhc.primary_event_ring().has_front() {
                    if let Err(err) = xhc.process_event() {
                        log!(LogLevel::Error, "Error while process_evnet: {}", err);
                    }
                }
            }
            MessageType::TimerTimeout { timeout, value } => {
                if value == textbox_cursor_timer {
                    TIMER_MANAGER.lock_wait().add_timer(Timer::new(
                        timeout + timer_05sec,
                        textbox_cursor_timer,
                        1,
                    ));
                    textbox_cursor_visible = !textbox_cursor_visible;
                    let mut layer_manager = loop {
                        match LAYER_MANAGER.lock() {
                            Some(manager) => break manager,
                            None => {
                                main_task.sleep();
                                continue;
                            }
                        }
                    };
                    draw_text_cursor(
                        textbox_cursor_visible,
                        text_window_index,
                        &mut layer_manager.layer(text_window_id).window().write(),
                    );
                    layer_manager.draw_id(text_window_id);
                }
            }
            MessageType::KeyPush {
                ascii,
                keycode,
                modifier,
                press,
            } => {
                if active == text_window_id {
                    if press {
                        // `input_text_window(ascii)` の代わり
                        'input_text_window: {
                            if ascii == 0 {
                                break 'input_text_window;
                            }

                            let pos = |index| Vector2D::new(4 + 8 * index, 6);

                            let mut manager = LAYER_MANAGER.lock_wait();
                            let window = manager.layer(text_window_id).window();
                            let mut window = window.write();

                            let max_chars = (window.width() as i32 - 8) / 8 - 1;
                            if ascii == 0x08 && text_window_index > 0 {
                                draw_text_cursor(false, text_window_index, &mut window);
                                text_window_index -= 1;
                                window.fill_rectangle(
                                    pos(text_window_index),
                                    Vector2D::new(8, 16),
                                    &PixelColor::to_color(0xffffff),
                                );
                                draw_text_cursor(true, text_window_index, &mut window);
                            } else if ascii >= b' ' && text_window_index < max_chars {
                                draw_text_cursor(false, text_window_index, &mut window);
                                font::write_ascii(
                                    &mut *window,
                                    pos(text_window_index),
                                    ascii,
                                    &PixelColor::to_color(0),
                                );
                                text_window_index += 1;
                                draw_text_cursor(true, text_window_index, &mut window);
                            }
                            manager.draw_id(text_window_id);
                        }
                    }
                }
                // F2
                else if press && keycode == 59 {
                    asmfunc::cli();
                    task::new_task()
                        .init_context(terminal::task_terminal, 0, 0)
                        .wake_up(-1);
                } else if let Some(task_id) = LAYER_TASK_MAP
                    .lock_wait()
                    .iter()
                    .find_map(|(&layer, &task)| if layer == active { Some(task) } else { None })
                {
                    asmfunc::cli();
                    let _ = task::send_message(
                        task_id,
                        Message {
                            ty: MessageType::KeyPush {
                                modifier,
                                keycode,
                                ascii,
                                press,
                            },
                            src_task: 1,
                        },
                    );
                    asmfunc::sti();
                } else {
                    printkln!(
                        "key push not handled: keycode {:02x}, ascii {:02x}",
                        keycode,
                        ascii
                    );
                    continue;
                }
            }
            MessageType::Layer {
                op,
                layer_id,
                pos,
                size,
            } => {
                layer::process_layer_message(op, layer_id, pos, size);
                let _ = task::send_message(msg.src_task, MessageType::LayerFinish.into());
            }
            _ => {}
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Write as _;
    cli();
    // エラーのたびに新しいインスタンスを作るので、最後に発生したエラーが表示される
    write!(&mut PanicConsole::new(), "{}", info).unwrap();
    halt()
}

#[cfg(debug_assertions)]
fn test_lib() {
    use alloc::string::String;

    use kernel::collections::HashMap;

    let mut map = HashMap::<String, _>::new();

    assert_eq!(map.cap(), 0);
    assert!(map.get("hoge").is_none());

    map.insert("hoge".into(), 1);
    assert_eq!(map.get("hoge").unwrap(), &1);
    assert_eq!(map.cap(), 16);

    map.insert("fuga".into(), 98);
    assert_eq!(map.get("fuga").unwrap(), &98);

    for i in 100..200 {
        map.insert(format!("{}", i), -i);
        if i < 178 {
            assert!(map.get("178").is_none());
        } else {
            assert_eq!(map.get("178").unwrap(), &-178);
        }
    }
    assert!(map.cap() >= 102);

    for i in (0..50).map(|i| 100 + i * 2) {
        assert_eq!(map.remove(&format!("{}", i)).unwrap(), -i);
    }

    log!(LogLevel::Info, "tests in test_lib() all succeeds");
}
