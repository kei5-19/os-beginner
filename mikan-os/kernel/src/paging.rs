use core::ops::{Index, IndexMut};

use crate::{asmfunc::set_cr3, sync::Mutex};

pub const PAGE_DIRECTORY_COUNT: usize = 64;

const PAGE_SIZE_4K: u64 = 4096;
const PAGE_SIZE_2M: u64 = 512 * PAGE_SIZE_4K;
const PAGE_SIZE_1G: u64 = 512 * PAGE_SIZE_2M;

static PML4_TABLE: Mutex<PageTable<u64, 512>> = Mutex::new(PageTable::<_, 512>::new(0));
static PDP_TABLE: Mutex<PageTable<u64, 512>> = Mutex::new(PageTable::<_, 512>::new(0));
static PAGE_DIRECTORY: Mutex<PageTable<[u64; 512], PAGE_DIRECTORY_COUNT>> =
    Mutex::new(PageTable::<_, PAGE_DIRECTORY_COUNT>::new([0; 512]));

pub fn init() {
    setup_indentity_page_table();
}

#[repr(align(4096))]
struct PageTable<T, const N: usize> {
    table: [T; N],
}

impl<T, const N: usize> PageTable<T, N> {
    fn len(&self) -> usize {
        N
    }

    fn as_ptr(&self) -> *const T {
        self.table.as_ptr()
    }
}

impl<T: Copy, const N: usize> PageTable<T, N> {
    const fn new(value: T) -> Self {
        Self { table: [value; N] }
    }
}

impl<T, const N: usize> Index<usize> for PageTable<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.table[index]
    }
}

impl<T, const N: usize> IndexMut<usize> for PageTable<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.table[index]
    }
}

pub fn setup_indentity_page_table() {
    let mut pml4_table = PML4_TABLE.lock();
    let mut pdp_table = PDP_TABLE.lock();
    let mut page_directory = PAGE_DIRECTORY.lock();

    pml4_table[0] = pdp_table.as_ptr() as u64 | 0x003;

    for i_pdpt in 0..page_directory.len() {
        pdp_table[i_pdpt] = page_directory[i_pdpt].as_ptr() as u64 | 0x003;

        for i_pd in 0..page_directory[0].len() {
            page_directory[i_pdpt][i_pd] =
                (i_pdpt as u64 * PAGE_SIZE_1G + i_pd as u64 * PAGE_SIZE_2M) | 0x083;
        }
    }

    set_cr3(pml4_table.as_ptr() as u64);
}
