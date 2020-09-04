use crate::{convert, mem, prims::*, traits::*, types::*};
use contracts_common::*;

use mem::MaybeUninit;

impl convert::From<()> for Reject {
    #[inline(always)]
    fn from(_: ()) -> Self { Reject {} }
}

/// # Contract state trait implementations.
impl Seek for ContractState {
    type Err = ();

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Err> {
        use core::convert::TryFrom;
        use SeekFrom::*;
        match pos {
            Start(offset) => match u32::try_from(offset) {
                Ok(offset_u32) => {
                    self.current_position = offset_u32;
                    Ok(offset)
                }
                _ => Err(()),
            },
            End(delta) => {
                let end = self.size();
                if delta >= 0 {
                    match u32::try_from(delta)
                        .ok()
                        .and_then(|x| self.current_position.checked_add(x))
                    {
                        Some(offset_u32) => {
                            self.current_position = offset_u32;
                            Ok(u64::from(offset_u32))
                        }
                        _ => Err(()),
                    }
                } else {
                    match delta.checked_abs().and_then(|x| u32::try_from(x).ok()) {
                        Some(before) if before <= end => {
                            let new_pos = end - before;
                            self.current_position = new_pos;
                            Ok(u64::from(new_pos))
                        }
                        _ => Err(()),
                    }
                }
            }
            Current(delta) => {
                let new_offset = if delta >= 0 {
                    u32::try_from(delta).ok().and_then(|x| self.current_position.checked_add(x))
                } else {
                    delta
                        .checked_abs()
                        .and_then(|x| u32::try_from(x).ok())
                        .and_then(|x| self.current_position.checked_sub(x))
                };
                match new_offset {
                    Some(offset) => {
                        self.current_position = offset;
                        Ok(u64::from(offset))
                    }
                    _ => Err(()),
                }
            }
        }
    }
}

impl Read for ContractState {
    type Err = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Err> {
        use core::convert::TryInto;
        let len: u32 = {
            match buf.len().try_into() {
                Ok(v) => v,
                _ => return Err(()),
            }
        };
        let num_read = unsafe { load_state(buf.as_mut_ptr(), len, self.current_position) };
        self.current_position += num_read;
        Ok(num_read as usize)
    }
    
    /// Read a `u32` in little-endian format. This is optimized to not
    /// initialize a dummy value before calling an external function.
    fn read_u64(&mut self) -> Result<u64, Self::Err> {
        let mut bytes: MaybeUninit<[u8; 8]> = MaybeUninit::uninit();
        let num_read =
        unsafe { load_state(bytes.as_mut_ptr() as *mut u8, 8, self.current_position) };
        self.current_position += num_read;
        if num_read == 8 {
            unsafe { Ok(u64::from_le_bytes(bytes.assume_init())) }
        } else {
            Err(())
        }
    }

    /// Read a `u32` in little-endian format. This is optimized to not
    /// initialize a dummy value before calling an external function.
    fn read_u32(&mut self) -> Result<u32, Self::Err> {
        let mut bytes: MaybeUninit<[u8; 4]> = MaybeUninit::uninit();
        let num_read =
        unsafe { load_state(bytes.as_mut_ptr() as *mut u8, 4, self.current_position) };
        self.current_position += num_read;
        if num_read == 4 {
            unsafe { Ok(u32::from_le_bytes(bytes.assume_init())) }
        } else {
            Err(())
        }
    }

    /// Read a `u8` in little-endian format. This is optimized to not
    /// initialize a dummy value before calling an external function.
    fn read_u8(&mut self) -> Result<u8, Self::Err> {
        let mut bytes: MaybeUninit<[u8; 1]> = MaybeUninit::uninit();
        let num_read =
            unsafe { load_state(bytes.as_mut_ptr() as *mut u8, 1, self.current_position) };
        self.current_position += num_read;
        if num_read == 1 {
            unsafe { Ok(bytes.assume_init()[0]) }
        } else {
            Err(())
        }
    }
}

impl Write for ContractState {
    type Err = ();

    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Err> {
        use core::convert::TryInto;
        let len: u32 = {
            match buf.len().try_into() {
                Ok(v) => v,
                _ => return Err(()),
            }
        };
        if self.current_position.checked_add(len).is_none() {
            return Err(());
        }
        let num_bytes = unsafe { write_state(buf.as_ptr(), len, self.current_position) };
        self.current_position += num_bytes; // safe because of check above that len + pos is small enough
        Ok(num_bytes as usize)
    }
}

impl HasContractState<()> for ContractState {
    type ContractStateData = ();

    #[inline(always)]
    fn open(_: Self::ContractStateData) -> Self {
        ContractState {
            current_position: 0,
        }
    }

    fn reserve(&mut self, len: u32) -> bool {
        let cur_size = unsafe { state_size() };
        if cur_size < len {
            let res = unsafe { resize_state(len) };
            res == 1
        } else {
            true
        }
    }

    #[inline(always)]
    fn size(&self) -> u32 { unsafe { state_size() } }

    fn truncate(&mut self, new_size: u32) {
        let cur_size = self.size();
        if cur_size > new_size {
            unsafe { resize_state(new_size) };
        }
        if new_size < self.current_position {
            self.current_position = new_size
        }
    }
}

/// # Trait implementations for Parameter
impl Read for Parameter {
    type Err = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Err> {
        use core::convert::TryInto;
        let len: u32 = {
            match buf.len().try_into() {
                Ok(v) => v,
                _ => return Err(()),
            }
        };
        let num_read =
            unsafe { get_parameter_section(buf.as_mut_ptr(), len, self.current_position) };
        self.current_position += num_read;
        Ok(num_read as usize)
    }
}

impl HasParameter for Parameter {
    #[inline(always)]
    fn size(&self) -> u32 { unsafe { get_parameter_size() } }
}

/// # Trait implementations for the chain metadata.
impl HasChainMetadata for ChainMetadata {
    #[inline(always)]
    fn slot_time(&self) -> SlotTime { self.slot_time }

    #[inline(always)]
    fn block_height(&self) -> BlockHeight { self.block_height }

    #[inline(always)]
    fn finalized_height(&self) -> FinalizedHeight { self.finalized_height }

    #[inline(always)]
    fn slot_number(&self) -> SlotNumber { self.slot_number }
}

/// # Trait implementations for the init context
impl HasInitContext<()> for InitContext {
    type InitData = ();
    type MetadataType = ChainMetadata;
    type ParamType = Parameter;

    /// Create a new init context by using an external call.
    fn open(_: Self::InitData) -> Self {
        let mut bytes = [0u8; 4 * 8 + 32];
        // unsafe { get_chain_context(bytes.as_mut_ptr()) }
        // unsafe { get_init_ctx(bytes[4 * 8..].as_mut_ptr()) };
        unsafe { get_init_ctx(bytes.as_mut_ptr()) };
        let mut cursor = Cursor::<&[u8]>::new(&bytes);
        if let Ok(v) = cursor.get() {
            v
        } else {
            panic!()
            // Host did not provide valid init context and chain metadata.
        }
    }

    #[inline(always)]
    fn init_origin(&self) -> &AccountAddress { &self.init_origin }

    #[inline(always)]
    fn parameter_cursor(&self) -> Self::ParamType {
        Parameter {
            current_position: 0,
        }
    }

    #[inline(always)]
    fn metadata(&self) -> &Self::MetadataType { &self.metadata }
}

/// # Trait implementations for the receive context
impl HasReceiveContext<()> for ReceiveContext {
    type MetadataType = ChainMetadata;
    type ParamType = Parameter;
    type ReceiveData = ();

    /// Create a new receive context by using an external call.
    fn open(_: Self::ReceiveData) -> Self {
        // let metadata_size = 4 * 8;
        // We reduce this to a purely stack-based allocation
        // by overapproximating the size of the context.
        // unsafe { get_receive_ctx_size() };
        let mut bytes = [0u8; 4 * 8 + 121];
        // unsafe { get_chain_context(bytes.as_mut_ptr()) }
        // unsafe { get_receive_ctx(bytes[metadata_size..].as_mut_ptr()) };
        unsafe { get_receive_ctx(bytes.as_mut_ptr()) };
        let mut cursor = Cursor::<&[u8]>::new(&bytes);
        if let Ok(v) = cursor.get() {
            v
        } else {
            panic!()
            // environment did not provide a valid receive context, this should
            // not happen and cannot be recovered.
        }
    }

    #[inline(always)]
    fn invoker(&self) -> &AccountAddress { &self.invoker }

    #[inline(always)]
    fn self_address(&self) -> &ContractAddress { &self.self_address }

    #[inline(always)]
    fn self_balance(&self) -> Amount { self.self_balance }

    #[inline(always)]
    fn sender(&self) -> &Address { &self.sender }

    #[inline(always)]
    fn owner(&self) -> &AccountAddress { &self.owner }

    #[inline(always)]
    fn parameter_cursor(&self) -> Self::ParamType {
        Parameter {
            current_position: 0,
        }
    }

    #[inline(always)]
    fn metadata(&self) -> &Self::MetadataType { &self.metadata }
}

/// #Implementations of the logger.

impl HasLogger for Logger {
    #[inline(always)]
    fn init() -> Self {
        Self {
            _private: (),
        }
    }

    #[inline(always)]
    fn log_bytes(&mut self, event: &[u8]) {
        unsafe {
            log_event(event.as_ptr(), event.len() as u32);
        }
    }
}

/// #Implementation of actions.
/// These actions are implemented by direct calls to host functions.
impl HasActions for Action {
    #[inline(always)]
    fn accept() -> Self {
        Action {
            _private: unsafe { accept() },
        }
    }

    #[inline(always)]
    fn simple_transfer(acc: &AccountAddress, amount: Amount) -> Self {
        let res = unsafe { simple_transfer(acc.0.as_ptr(), amount) };
        Action {
            _private: res,
        }
    }

    #[inline(always)]
    fn send(ca: &ContractAddress, receive_name: &str, amount: Amount, parameter: &[u8]) -> Self {
        let receive_bytes = receive_name.as_bytes();
        let res = unsafe {
            send(
                ca.index,
                ca.subindex,
                receive_bytes.as_ptr(),
                receive_bytes.len() as u32,
                amount,
                parameter.as_ptr(),
                parameter.len() as u32,
            )
        };
        Action {
            _private: res,
        }
    }

    #[inline(always)]
    fn and_then(self, then: Self) -> Self {
        let res = unsafe { combine_and(self._private, then._private) };
        Action {
            _private: res,
        }
    }

    #[inline(always)]
    fn or_else(self, el: Self) -> Self {
        let res = unsafe { combine_or(self._private, el._private) };
        Action {
            _private: res,
        }
    }
}
