use crate::util::vec_exists;
use crate::vm::PciSlot;

#[derive(Debug)]
pub struct PciSlotGenerator {
    bus: u8,
    slot: u8,
    skip: Vec<PciSlot>,
}

impl PciSlotGenerator {
    pub fn build(bus: u8, slot: u8, skip: Vec<PciSlot>) -> PciSlotGenerator {
        Self { bus, slot, skip }
    }

    pub fn try_take_specific_bus(&mut self, bus: u8) -> Option<PciSlot> {
        if self.bus > bus {
            None
        } else if self.bus == bus {
            self.next_slot()
        } else {
            /* if self.bus < bus, we just issue them the first slot of the
             * requested bus
             */
            let mut slot = PciSlot {
                bus,
                slot: 0,
                func: 0,
            };

            while self.skip.contains(&slot) {
                if slot.slot == 31 {
                    /* oops, we run out of the slots possible! */
                    return None;
                }
                slot.slot += 1;
            }

            self.skip.push(slot);
            Some(slot)
        }
    }

    #[allow(dead_code)]
    pub fn try_take_specific_bus_slot(&mut self, bus: u8, slot: u8) -> Option<PciSlot> {
        /* first check if if it's in any skipped one
         * if the local bus state is greated than requested,
         * we either can't issue it or already issued;
         * similarly, if self.bus == bus, we check if local slot state is greater.
         */
        if vec_exists(&self.skip, |s| s.bus == bus && s.slot == slot)
            || self.bus > bus
            || (self.bus == bus && self.slot > slot)
        {
            None
        } else if self.bus == bus && self.slot == slot {
            /* if it turns out the requested slot is the next slot, issue it
             * as if we are just giving out the next slot
             */
            self.next_slot()
        } else {
            let slot = PciSlot { bus, slot, func: 0 };
            self.skip.push(slot);
            Some(slot)
        }
    }

    pub fn next_slot(&mut self) -> Option<PciSlot> {
        if self.bus == 255 && self.slot == 31 {
            None
        } else {
            if self.slot == 31 {
                self.bus += 1;
                self.slot = 0;
                self.next_slot()
            } else {
                let ret = PciSlot {
                    bus: self.bus,
                    slot: self.slot,
                    func: 0,
                };
                self.slot += 1;

                if self.skip.contains(&ret) {
                    self.next_slot()
                } else {
                    Some(ret)
                }
            }
        }
    }
}
