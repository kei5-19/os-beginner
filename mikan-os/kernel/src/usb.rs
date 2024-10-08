use core::{
    ffi::{c_char, c_int, c_schar, c_uchar, c_ulong, c_void, CStr},
    mem::MaybeUninit,
};

use crate::error::{self, Error, Result};

extern "C" {
    #[link_name = "_ZN3usb4xhci10ControllerC2Em"]
    fn contoller(this: *mut Controller, mmio_base: c_ulong);

    #[link_name = "_ZN3usb4xhci10Controller10InitializeEv"]
    fn contoller_initialize(this: *mut Controller) -> CxxError;

    #[link_name = "_ZN3usb4xhci10Controller3RunEv"]
    fn controller_run(this: *mut Controller) -> CxxError;

    #[link_name = "_ZN3usb4xhci10Controller6PortAtEh"]
    fn controller_port_at(this: *mut Controller, port_num: c_uchar) -> Port;

    #[link_name = "_ZN3usb4xhci10Controller16PrimaryEventRingEv"]
    fn controller_primay_event_ring(this: *mut Controller) -> *mut EventRing;

    #[link_name = "_ZN3usb4xhci13ConfigurePortERNS0_10ControllerERNS0_4PortE"]
    fn xhci_configure_port(xhc: *mut Controller, port: *mut Port) -> CxxError;

    #[link_name = "_ZN3usb4xhci12ProcessEventERNS0_10ControllerE"]
    fn xhci_process_event(xhc: *mut Controller) -> CxxError;

    #[link_name = "_ZN3usb14HIDMouseDriver18SetDefaultObserverEPFvhaaE"]
    fn hid_mouse_driver_set_default_observer(observer: *const c_void);

    #[link_name = "_ZNK3usb4xhci4Port11IsConnectedEv"]
    fn port_is_connected(this: *const Port) -> bool;

    #[link_name = "_ZNK3usb4xhci9EventRing8HasFrontEv"]
    fn event_ring_has_front(this: *const EventRing) -> bool;

    #[link_name = "_ZN3usb17HIDKeyboardDriver18SetDefaultObserverEPFvhhbE"]
    fn hid_keyboard_driver_set_default_observer(observer: *const c_void);
}

#[repr(C)]
pub struct Controller {
    mmio_base: c_ulong,
    cap: *const (),
    op: *const (),
    max_ports: c_uchar,
    devmgr: DeviceManager,
    cr: Ring,
    er: EventRing,
}

unsafe impl Send for Controller {}

impl Controller {
    pub fn new(mmio_base: u64) -> Self {
        let mut this = MaybeUninit::<Controller>::uninit();
        unsafe {
            contoller(this.as_mut_ptr(), mmio_base);
            this.assume_init()
        }
    }

    pub fn initialize(&mut self) -> Result<()> {
        unsafe { contoller_initialize(self as *mut Self) }.into()
    }

    pub fn run(&mut self) -> Result<()> {
        unsafe { controller_run(self as *mut Self) }.into()
    }

    pub fn max_ports(&self) -> u8 {
        self.max_ports
    }

    pub fn port_at(&mut self, port_num: u8) -> Port {
        unsafe { controller_port_at(self as *mut Self, port_num) }
    }

    pub fn configure_port(&mut self, port: &mut Port) -> Result<()> {
        unsafe { xhci_configure_port(self as *mut Self, port as *mut Port) }.into()
    }

    pub fn process_event(&mut self) -> Result<()> {
        unsafe { xhci_process_event(self as *mut Self) }.into()
    }

    pub fn primary_event_ring(&mut self) -> &mut EventRing {
        unsafe {
            controller_primay_event_ring(self as *mut Self)
                .as_mut()
                .unwrap()
        }
    }
}

#[repr(C)]
pub struct DeviceManager {
    device_context_pointers: *mut *mut (), // 本当は DeviceContext**
    max_slots: c_ulong,
    devices: *mut *mut (), // 本当は Device**
}

#[repr(C)]
pub struct Ring {
    buf: *mut (), // 本当は *TRB
    buf_size: c_ulong,
    cycle_bit: bool,
    write_index: c_ulong,
}

#[repr(C)]
pub struct EventRing {
    buf: *mut (), // 本当は TRB*
    buf_size: c_ulong,
    cycle_bit: bool,
    erst: *mut (),        // 本当は EventRingSegmentTableEntry*
    interrupter: *mut (), // 本当は InterrupterRegisterSet*
}

impl EventRing {
    pub fn has_front(&self) -> bool {
        unsafe { event_ring_has_front(self as *const Self) }
    }
}

type MouseObserverType = fn(c_uchar, c_schar, c_schar);

#[repr(C)]
struct Function {
    a: [i32; 8], // 本当の構成は分からないが、32 byte 使っているのでこうしている
}

#[repr(C)]
pub struct HIDMouseDriver {
    observers: [Function; 4], // 本当は Function<MouseObserverType>
    num_observers: i32,
}

impl HIDMouseDriver {
    pub fn set_default_observer(observer: MouseObserverType) {
        unsafe {
            hid_mouse_driver_set_default_observer(observer as *const c_void);
        }
    }
}

type KeyboardObserverType = fn(c_uchar, c_uchar, bool);

#[repr(C)]
pub struct HIDKeyboardDriver {
    observers: [Function; 4], // 本当は Function<KeyboardObserverType>
    num_observers: i32,
}

impl HIDKeyboardDriver {
    pub fn set_default_observer(observer: KeyboardObserverType) {
        unsafe {
            hid_keyboard_driver_set_default_observer(observer as *const c_void);
        }
    }
}

/// C++ 版の [Code][error::Code] に対応する列挙型。
// 以下の構造体は C++ 側からしか使われないため、Rust 側では使わない
#[repr(C)]
#[allow(unused)]
enum CxxCode {
    Success,
    Full,
    Empty,
    NoEnoughMemory,
    IndexOutOfRange,
    HostControllerNotHalted,
    InvalidSlotID,
    PortNotConnected,
    InvalidEndpointNumber,
    TransferRingNotSet,
    AlreadyAllocated,
    NotImplemented,
    InvalidDescriptor,
    BufferTooSmall,
    UnknownDevice,
    NoCorrespondingSetupStage,
    TransferFailed,
    InvalidPhase,
    UnknownXHCISpeedID,
    NoWaiter,
    NoPCIMSI,
    UnknownPixelFormat,
    NoSuchTask,
    InvalidFormat,
    FrameTooSmall,
    InvalidFile,
    IsDirectory,
    NoSuchEntry,
    FreeTypeError,
    EndpointNotInCharge,
}

impl From<CxxCode> for error::Code {
    /// C++ 版の [CxxCode] から Rust 版の [Code][error::Code] に変換する。
    ///
    /// # Note
    /// この関数は、[CxxCode::Success] は変換できない。
    fn from(value: CxxCode) -> Self {
        match value {
            CxxCode::Success => panic!("CxxCode::Success cannot be converted into error::Code"),
            CxxCode::Full => error::Code::Full,
            CxxCode::Empty => error::Code::Empty,
            CxxCode::NoEnoughMemory => error::Code::NoEnoughMemory,
            CxxCode::IndexOutOfRange => error::Code::IndexOutOfRange,
            CxxCode::HostControllerNotHalted => error::Code::HostControllerNotHalted,
            CxxCode::InvalidSlotID => error::Code::InvalidSlotID,
            CxxCode::PortNotConnected => error::Code::PortNotConnected,
            CxxCode::InvalidEndpointNumber => error::Code::InvalidEndpointNumber,
            CxxCode::TransferRingNotSet => error::Code::TransferRingNotSet,
            CxxCode::AlreadyAllocated => error::Code::AlreadyAllocated,
            CxxCode::NotImplemented => error::Code::NotImplemented,
            CxxCode::InvalidDescriptor => error::Code::InvalidDescriptor,
            CxxCode::BufferTooSmall => error::Code::BufferTooSmall,
            CxxCode::UnknownDevice => error::Code::UnknownDevice,
            CxxCode::NoCorrespondingSetupStage => error::Code::NoCorrespondingSetupStage,
            CxxCode::TransferFailed => error::Code::TransferFailed,
            CxxCode::InvalidPhase => error::Code::InvalidPhase,
            CxxCode::UnknownXHCISpeedID => error::Code::UnknownXHCISpeedID,
            CxxCode::NoWaiter => error::Code::NoWaiter,
            CxxCode::NoPCIMSI => error::Code::NoPCIMSI,
            CxxCode::UnknownPixelFormat => error::Code::UnknownPixelFormat,
            CxxCode::NoSuchTask => error::Code::NoSuchTask,
            CxxCode::InvalidFormat => error::Code::InvalidFormat,
            CxxCode::FrameTooSmall => error::Code::FrameTooSmall,
            CxxCode::InvalidFile => error::Code::InvalidFile,
            CxxCode::IsDirectory => error::Code::IsDirectory,
            CxxCode::NoSuchEntry => error::Code::NoSuchEntry,
            CxxCode::FreeTypeError => error::Code::FreeTypeError,
            CxxCode::EndpointNotInCharge => error::Code::EndpointNotInCharge,
        }
    }
}

/// C++ 版の [Error] に対応する構造体。
#[repr(C)]
struct CxxError {
    code: CxxCode,
    line: c_int,
    file: *const c_char,
}

impl From<CxxError> for Result<()> {
    /// C++ 版の [CxxError] から Rust 版の [Result] に変換する。
    fn from(value: CxxError) -> Self {
        match value.code {
            CxxCode::Success => Ok(()),
            _ => Err(Error::new(
                value.code.into(),
                "",
                unsafe { CStr::from_ptr(value.file) }.to_str().unwrap(),
                value.line as u32,
            )),
        }
    }
}

#[repr(C)]
pub struct Port {
    port_num: c_uchar,
    port_reg_set: *mut (), // 本当は PortRegisterSet&
}

impl Port {
    pub fn is_connected(&self) -> bool {
        unsafe { port_is_connected(self as *const Self) }
    }
}
