//! A firmware smoke test for the HII producer and consumer.
//!
//! Build it for a UEFI target and boot it in QEMU. The program publishes one
//! formset through [`hii_uefi::publish::Registration`] and then reads and
//! writes the formset through [`hii_uefi::hii::HiiProvider`]. The serial
//! console shows `SMOKE PASS` on success.
//!
//! ```sh
//! cargo build -p hii-uefi --example smoke --target x86_64-unknown-uefi
//! mkdir -p esp/EFI/BOOT
//! cp out/cargo/x86_64-unknown-uefi/debug/examples/smoke.efi esp/EFI/BOOT/BOOTX64.EFI
//! dd if=/dev/zero of=smoke.img bs=1M count=32
//! mkfs.vfat smoke.img
//! mmd -i smoke.img ::/EFI ::/EFI/BOOT
//! mcopy -i smoke.img esp/EFI/BOOT/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI
//! cp /usr/share/OVMF/OVMF_VARS_4M.fd smoke-vars.fd
//! qemu-system-x86_64 -machine q35 -m 256 \
//!   -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
//!   -drive if=pflash,format=raw,file=smoke-vars.fd \
//!   -drive format=raw,file=smoke.img -display none -serial stdio -no-reboot
//! ```
//!
//! The image shows `SMOKE PASS` on the serial console and powers off.
#![cfg_attr(target_os = "uefi", no_std)]
#![cfg_attr(target_os = "uefi", no_main)]

#[cfg(not(target_os = "uefi"))]
fn main() {}

#[cfg(target_os = "uefi")]
mod smoke {
    extern crate alloc;

    use alloc::collections::BTreeMap;
    use alloc::rc::Rc;
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;
    use core::alloc::{GlobalAlloc, Layout};
    use core::cell::RefCell;
    use core::panic::PanicInfo;
    use core::ptr::{self, NonNull};
    use core::sync::atomic::{AtomicPtr, Ordering};

    use forms::{
        ActionRequest, Error, Form, FormId, FormSet, Provider as FormsProvider, Question,
        QuestionFlags, QuestionId, Statement, Storage, Value, Width,
    };
    use hii_uefi::hii::HiiProvider;
    use hii_uefi::publish::Registration;
    use hii_uefi::raw::Boot;
    use uefi_raw::table::boot::{BootServices, MemoryType};
    use uefi_raw::table::runtime::ResetType;
    use uefi_raw::table::system::SystemTable;
    use uefi_raw::{Handle, Status};

    /// The boot services that the global allocator uses.
    static SERVICES: AtomicPtr<BootServices> = AtomicPtr::new(ptr::null_mut());

    /// Allocates from firmware pool memory, so the example needs no heap.
    struct FirmwareHeap;

    // SAFETY: Every allocation comes from `AllocatePool` and carries its base
    // pointer in the eight bytes before the returned address.
    unsafe impl GlobalAlloc for FirmwareHeap {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let services = SERVICES.load(Ordering::Acquire);
            if services.is_null() {
                return ptr::null_mut();
            }
            let align = layout.align().max(8);
            let Some(total) = layout.size().checked_add(align + 8) else {
                return ptr::null_mut();
            };
            let mut base: *mut u8 = ptr::null_mut();
            // SAFETY: The boot services pointer came from the system table.
            let status = unsafe {
                ((*services).allocate_pool)(MemoryType::BOOT_SERVICES_DATA, total, &mut base)
            };
            if status.is_error() || base.is_null() {
                return ptr::null_mut();
            }
            let start = (((base as usize) + 8 + align - 1) & !(align - 1)) as *mut u8;
            // SAFETY: The header lies inside the allocation.
            unsafe { (start.sub(8) as *mut usize).write(base as usize) };
            start
        }

        unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
            let services = SERVICES.load(Ordering::Acquire);
            if services.is_null() {
                return;
            }
            // SAFETY: `alloc` wrote the base pointer there.
            let base = unsafe { (pointer.sub(8) as *const usize).read() } as *mut u8;
            // SAFETY: The base pointer came from `AllocatePool`.
            unsafe {
                let _ = ((*services).free_pool)(base);
            }
        }
    }

    #[global_allocator]
    static HEAP: FirmwareHeap = FirmwareHeap;

    /// The console that the example writes to.
    static STDOUT: AtomicPtr<uefi_raw::protocol::console::SimpleTextOutputProtocol> =
        AtomicPtr::new(ptr::null_mut());

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        print("SMOKE PANIC\r\n");
        loop {}
    }

    /// Prints one line to the UEFI console without an allocation.
    fn print(text: &str) {
        let mut units = [0u16; 256];
        let mut length = 0;
        for (index, unit) in text.encode_utf16().enumerate() {
            if index + 1 >= units.len() {
                break;
            }
            units[index] = unit;
            length = index + 1;
        }
        units[length] = 0;
        let stdout = STDOUT.load(Ordering::Acquire);
        if stdout.is_null() {
            return;
        }
        // SAFETY: The system table owns the console protocol.
        unsafe {
            let _ = ((*stdout).output_string)(stdout, units.as_ptr());
        }
    }

    /// Returns a question with no storage. The producer maps it.
    fn question(id: u16) -> Question {
        Question {
            id: QuestionId(id),
            prompt: "Value".to_string(),
            help: String::new(),
            storage: Storage::None,
            flags: QuestionFlags(0),
        }
    }

    /// Builds the sample formset.
    fn formset() -> FormSet {
        FormSet {
            guid: [
                0x42, 0x51, 0x33, 0x24, 0x15, 0x06, 0xF7, 0xE8, 0xD9, 0xCA, 0xBB, 0xAC, 0x9D, 0x8E,
                0x7F, 0x60,
            ],
            title: "Smoke formset".to_string(),
            help: String::new(),
            class_guids: Vec::new(),
            varstores: Vec::new(),
            forms: vec![Form {
                id: FormId(1),
                title: "Main".to_string(),
                items: vec![
                    Statement::Checkbox {
                        question: question(1),
                        default: true,
                    }
                    .into(),
                    Statement::Numeric {
                        question: question(2),
                        width: Width::U16,
                        minimum: 0,
                        maximum: 1000,
                        step: 1,
                    }
                    .into(),
                ],
            }],
        }
    }

    /// The in-memory settings of the example.
    #[derive(Default)]
    struct Settings {
        values: BTreeMap<u16, Value>,
        writes: Vec<(u16, Value)>,
    }

    /// A provider over [`Settings`], shared with the test body.
    struct MemoryProvider(Rc<RefCell<Settings>>);

    impl FormsProvider for MemoryProvider {
        type Handle = u16;

        fn formsets(&mut self) -> Result<Vec<(Self::Handle, FormSet)>, Error> {
            Ok(Vec::new())
        }

        fn read(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
        ) -> Result<Value, Error> {
            Ok(self
                .0
                .borrow()
                .values
                .get(&question.id.0)
                .cloned()
                .unwrap_or(Value::Number(0)))
        }

        fn write(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            question: &Question,
            value: &Value,
        ) -> Result<ActionRequest, Error> {
            let mut settings = self.0.borrow_mut();
            settings.values.insert(question.id.0, value.clone());
            settings.writes.push((question.id.0, value.clone()));
            Ok(ActionRequest::None)
        }

        fn action(
            &mut self,
            _handle: Self::Handle,
            _set: &FormSet,
            _question: &Question,
        ) -> Result<ActionRequest, Error> {
            Err(Error {
                code: Status::UNSUPPORTED.0 as u64,
                reason: "no actions".to_string(),
            })
        }
    }

    /// Runs the smoke test and returns a failure message.
    fn run(boot: Boot) -> Result<(), String> {
        let settings = Rc::new(RefCell::new(Settings::default()));
        settings.borrow_mut().values.insert(1, Value::Bool(true));
        settings.borrow_mut().values.insert(2, Value::Number(7));
        let provider = MemoryProvider(settings.clone());

        // SAFETY: The program stays resident until removal.
        let mut registration = unsafe {
            Registration::new(boot, formset(), provider, 0)
                .map_err(|status| alloc::format!("registration failed: {status}"))?
        };
        print("SMOKE published\r\n");

        let mut consumer = HiiProvider::new(boot)
            .map_err(|error| alloc::format!("consumer failed: {}", error.reason))?;
        let sets = consumer
            .formsets()
            .map_err(|error| alloc::format!("cannot list formsets: {}", error.reason))?;
        let (handle, set) = sets
            .iter()
            .find(|(_, set)| set.guid == formset().guid)
            .map(|(handle, set)| (*handle, set.clone()))
            .ok_or_else(|| "the published formset is missing".to_string())?;
        print("SMOKE listed formset\r\n");

        let numeric = set
            .forms
            .iter()
            .flat_map(|form| form.items.iter())
            .find_map(|item| match &item.statement {
                Statement::Numeric { question, .. } if question.id.0 == 2 => Some(question.clone()),
                _ => None,
            })
            .ok_or_else(|| "the numeric question is missing".to_string())?;

        let value = consumer
            .read(handle, &set, &numeric)
            .map_err(|error| alloc::format!("read failed: {}", error.reason))?;
        if value != Value::Number(7) {
            return Err(alloc::format!("read returned {value:?}"));
        }
        print("SMOKE read value\r\n");

        consumer
            .write(handle, &set, &numeric, &Value::Number(42))
            .map_err(|error| alloc::format!("write failed: {}", error.reason))?;
        let observed = settings.borrow().values.get(&2).cloned();
        if observed != Some(Value::Number(42)) {
            return Err(alloc::format!("provider saw {observed:?}"));
        }
        print("SMOKE provider saw the write\r\n");

        registration
            .remove()
            .map_err(|status| alloc::format!("removal failed: {status}"))?;
        print("SMOKE removed\r\n");
        Ok(())
    }

    /// Entry point of the example.
    #[unsafe(no_mangle)]
    pub extern "efiapi" fn efi_main(_image: Handle, table: *mut SystemTable) -> Status {
        // SAFETY: Firmware passes a live system table.
        let table = unsafe { &*table };
        SERVICES.store(table.boot_services, Ordering::Release);
        STDOUT.store(table.stdout, Ordering::Release);
        print("SMOKE entry\r\n");
        // SAFETY: The boot services table lives until ExitBootServices.
        let boot = unsafe {
            Boot::from_system_table(NonNull::new(table as *const _ as *mut _).expect("table"))
        }
        .expect("boot services");
        print("SMOKE boot services\r\n");

        let result = run(boot);
        match &result {
            Ok(()) => print("SMOKE PASS\r\n"),
            Err(reason) => {
                let message = alloc::format!("SMOKE FAIL {reason}\r\n");
                print(&message);
            }
        }

        // SAFETY: Runtime services stay valid after driver return.
        unsafe {
            ((*table.runtime_services).reset_system)(
                ResetType::SHUTDOWN,
                Status::SUCCESS,
                0,
                ptr::null(),
            )
        }
    }
}
