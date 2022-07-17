use bitflags::bitflags;
use core::{mem::size_of, ptr::null_mut};

extern "C" {
    static HEAP_START: usize;
    static HEAP_SIZE: usize;
}

// We will use ALLOC_START to mark the start of the actual
// memory we can dish out.
static mut ALLOC_START: usize = 0;
const PAGE_ORDER: usize = 12;
pub const PAGE_SIZE: usize = 1 << 12;

bitflags! {
    pub struct PageFlags: u8 {
        const TAKEN = 1 << 0;
        const LAST = 1 << 1;
        const TAKEN_LAST = Self::TAKEN.bits | Self::LAST.bits;
    }
}

pub struct Page {
    flags: PageFlags,
}

impl Page {
    pub fn is_last(&self) -> bool {
        self.flags.contains(PageFlags::LAST)
    }

    // If the page is marked as being taken (allocated), then
    // this function returns true. Otherwise, it returns false.
    pub fn is_taken(&self) -> bool {
        self.flags.contains(PageFlags::TAKEN)
    }

    // This is the opposite of is_taken().
    pub fn is_free(&self) -> bool {
        !self.is_taken()
    }

    // Clear the Page structure and all associated allocations.
    pub fn clear(&mut self) {
        self.flags = PageFlags::empty();
    }

    pub fn alloc(&mut self) {
        self.flags = PageFlags::TAKEN;
    }

    pub fn alloc_last(&mut self) {
        self.flags = PageFlags::TAKEN_LAST;
    }
}

/// Initialize the allocation system. There are several ways that we can
/// implement the page allocator:
/// 1. Free list (singly linked list where it starts at the first free
/// allocation) 2. Bookkeeping list (structure contains a taken and length)
/// 3. Allocate one Page structure per 4096 bytes (this is what I chose)
/// 4. Others
pub fn init() {
    unsafe {
        let num_pages = HEAP_SIZE / PAGE_SIZE;
        let ptr = HEAP_START as *mut Page;
        // Clear all pages to make sure that they aren't accidentally
        // taken
        for i in 0..num_pages {
            (*ptr.add(i)).clear();
        }
        // Determine where the actual useful memory starts. This will be
        // after all Page structures. We also must align the ALLOC_START
        // to a page-boundary (PAGE_SIZE = 4096). ALLOC_START =
        // (HEAP_START + num_pages * size_of::<Page>() + PAGE_SIZE - 1)
        // & !(PAGE_SIZE - 1);
        ALLOC_START = align_val(HEAP_START + num_pages * size_of::<Page>(), PAGE_ORDER);
    }
}

pub const fn align_val(val: usize, order: usize) -> usize {
    let o = (1usize << order) - 1;
    (val + o) & !o
}

/// Allocate a page or multiple pages
/// pages: the number of PAGE_SIZE pages to allocate
pub fn alloc(pages: usize) -> *mut u8 {
    // We have to find a contiguous allocation of pages
    assert!(pages > 0);
    unsafe {
        // We create a Page structure for each page on the heap. We
        // actually might have more since HEAP_SIZE moves and so does
        // the size of our structure, but we'll only waste a few bytes.
        let num_pages = HEAP_SIZE / PAGE_SIZE;
        let ptr = HEAP_START as *mut Page;
        for i in 0..num_pages - pages {
            let mut found = false;
            // Check to see if this Page is free. If so, we have our
            // first candidate memory address.
            if (*ptr.add(i)).is_free() {
                // It was FREE! Yay!
                found = true;
                for j in i..i + pages {
                    // Now check to see if we have a
                    // contiguous allocation for all of the
                    // request pages. If not, we should
                    // check somewhere else.
                    if (*ptr.add(j)).is_taken() {
                        found = false;
                        break;
                    }
                }
            }
            // We've checked to see if there are enough contiguous
            // pages to form what we need. If we couldn't, found
            // will be false, otherwise it will be true, which means
            // we've found valid memory we can allocate.
            if found {
                for k in i..i + pages - 1 {
                    (*ptr.add(k)).alloc();
                }
                // The marker for the last page is
                // PageBits::Last This lets us know when we've
                // hit the end of this particular allocation.
                (*ptr.add(i + pages - 1)).alloc_last();
                // The Page structures themselves aren't the
                // useful memory. Instead, there is 1 Page
                // structure per 4096 bytes starting at
                // ALLOC_START.
                return (ALLOC_START + PAGE_SIZE * i) as *mut u8;
            }
        }
    }

    // If we get here, that means that no contiguous allocation was
    // found.
    null_mut()
}

/// Allocate and zero a page or multiple pages
/// pages: the number of pages to allocate
/// Each page is PAGE_SIZE which is calculated as 1 << PAGE_ORDER
/// On RISC-V, this typically will be 4,096 bytes.
pub fn zalloc(pages: usize) -> *mut u8 {
    // Allocate and zero a page.
    // First, let's get the allocation
    let ret = alloc(pages);
    if !ret.is_null() {
        let size = (PAGE_SIZE * pages) / 8;
        let big_ptr = ret as *mut u64;
        for i in 0..size {
            // We use big_ptr so that we can force an
            // sd (store doubleword) instruction rather than
            // the sb. This means 8x fewer stores than before.
            // Typically we have to be concerned about remaining
            // bytes, but fortunately 4096 % 8 = 0, so we
            // won't have any remaining bytes.
            unsafe {
                (*big_ptr.add(i)) = 0;
            }
        }
    }
    ret
}

/// Deallocate a page by its pointer
/// The way we've structured this, it will automatically coalesce
/// contiguous pages.
pub fn dealloc(ptr: *mut u8) {
    // Make sure we don't try to free a null pointer.
    assert!(!ptr.is_null());
    unsafe {
        let addr = HEAP_START + (ptr as usize - ALLOC_START) / PAGE_SIZE;
        // Make sure that the address makes sense. The address we
        // calculate here is the page structure, not the HEAP address!
        assert!(addr >= HEAP_START && addr < HEAP_START + HEAP_SIZE);
        let mut p = addr as *mut Page;
        // Keep clearing pages until we hit the last page.
        while (*p).is_taken() && !(*p).is_last() {
            (*p).clear();
            p = p.add(1);
        }
        // If the following assertion fails, it is most likely
        // caused by a double-free.
        assert!(
            (*p).is_last(),
            "Possible double-free detected! (Not taken found \
		         before last)"
        );
        // If we get here, we've taken care of all previous pages and
        // we are on the last page.
        (*p).clear();
    }
}

/// Print all page allocations
/// This is mainly used for debugging.
pub fn print_page_allocations() {
    unsafe {
        let num_pages = HEAP_SIZE / PAGE_SIZE;
        let mut beg = HEAP_START as *const Page;
        let end = beg.add(num_pages);
        let alloc_beg = ALLOC_START;
        let alloc_end = ALLOC_START + num_pages * PAGE_SIZE;
        println!();
        println!(
            "PAGE ALLOCATION TABLE\nMETA: {:p} -> {:p}\nPHYS: \
					0x{:x} -> 0x{:x}",
            beg, end, alloc_beg, alloc_end
        );
        println!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~");
        let mut num = 0;
        while beg < end {
            if (*beg).is_taken() {
                let start = beg as usize;
                let memaddr = ALLOC_START + (start - HEAP_START) * PAGE_SIZE;
                print!("0x{:x} => ", memaddr);
                loop {
                    num += 1;
                    if (*beg).is_last() {
                        let end = beg as usize;
                        let memaddr = ALLOC_START + (end - HEAP_START) * PAGE_SIZE + PAGE_SIZE - 1;
                        print!("0x{:x}: {:>3} page(s)", memaddr, (end - start + 1));
                        println!(".");
                        break;
                    }
                    beg = beg.add(1);
                }
            }
            beg = beg.add(1);
        }
        println!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~");
        println!(
            "Allocated: {:>6} pages ({:>10} bytes).",
            num,
            num * PAGE_SIZE
        );
        println!(
            "Free     : {:>6} pages ({:>10} bytes).",
            num_pages - num,
            (num_pages - num) * PAGE_SIZE
        );
        println!();
    }
}
