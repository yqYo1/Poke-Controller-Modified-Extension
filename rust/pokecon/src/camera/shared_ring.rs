use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::camera::frame::{BgrFrame, CaptureResolution, FrameError, FrameSize, MAX_FRAME_BYTES};

pub const SLOT_COUNT: usize = 3;
pub const SLOT_BYTE_SIZE: usize = MAX_FRAME_BYTES;
pub const INVALID_PUBLISHED_TOKEN: u64 = u64::MAX;
pub const MAX_FRAME_SEQUENCE: u64 = (1_u64 << 62) - 2;
pub const READ_RETRY_LIMIT: usize = 8;

const SLOT_EMPTY: u32 = 0;
const SLOT_WRITING: u32 = 1;
const SLOT_PUBLISHED: u32 = 2;
const DTYPE_UINT8: [u8; 8] = *b"uint8\0\0\0";

/// Stable descriptor sent once when a worker maps the lifetime-fixed ring.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingDescriptor {
    pub shm_handle: String,
    pub total_size: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub slot_byte_size: u64,
}

/// One successful latest-frame publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Publication {
    pub slot_index: usize,
    pub frame_sequence: u64,
    pub size: FrameSize,
}

/// Three-slot persistent shared-memory latest-frame ring.
#[derive(Clone)]
pub struct SharedFrameRing {
    mapping: Arc<mapping::MappedRing>,
    descriptor: MappingDescriptor,
    next_sequence: Arc<AtomicU64>,
}

impl std::fmt::Debug for SharedFrameRing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedFrameRing")
            .field("total_size", &self.descriptor.total_size)
            .field("slot_byte_size", &self.descriptor.slot_byte_size)
            .field("published_token", &self.published_token())
            .finish_non_exhaustive()
    }
}

impl SharedFrameRing {
    /// Creates and initializes a new owner mapping.
    ///
    /// # Errors
    ///
    /// Returns a fixed error if the named OS mapping cannot be created.
    pub fn create(initial_resolution: CaptureResolution) -> Result<Self, RingError> {
        let mapping = Arc::new(mapping::MappedRing::create()?);
        let initial_size = initial_resolution.size();
        let descriptor = MappingDescriptor {
            shm_handle: mapping.os_id().to_owned(),
            total_size: u64::try_from(mapping::TOTAL_SIZE)
                .map_err(|_| RingError::InvalidDescriptor)?,
            frame_width: initial_size.width(),
            frame_height: initial_size.height(),
            slot_byte_size: u64::try_from(SLOT_BYTE_SIZE)
                .map_err(|_| RingError::InvalidDescriptor)?,
        };
        Ok(Self {
            mapping,
            descriptor,
            next_sequence: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Opens an existing owner-created mapping.
    ///
    /// # Errors
    ///
    /// Rejects descriptors whose fixed layout or initial dimensions differ.
    pub fn open(descriptor: MappingDescriptor) -> Result<Self, RingError> {
        let total_size =
            usize::try_from(descriptor.total_size).map_err(|_| RingError::InvalidDescriptor)?;
        let slot_byte_size =
            usize::try_from(descriptor.slot_byte_size).map_err(|_| RingError::InvalidDescriptor)?;
        FrameSize::new(descriptor.frame_width, descriptor.frame_height)?;
        if total_size != mapping::TOTAL_SIZE || slot_byte_size != SLOT_BYTE_SIZE {
            return Err(RingError::InvalidDescriptor);
        }
        let mapping = Arc::new(mapping::MappedRing::open(
            &descriptor.shm_handle,
            total_size,
        )?);
        let next_sequence = decode_token(mapping.load_published_token(Ordering::Acquire))?.map_or(
            0,
            |(_, sequence)| {
                if sequence >= MAX_FRAME_SEQUENCE {
                    0
                } else {
                    sequence + 1
                }
            },
        );
        Ok(Self {
            mapping,
            descriptor,
            next_sequence: Arc::new(AtomicU64::new(next_sequence)),
        })
    }

    #[must_use]
    pub fn descriptor(&self) -> MappingDescriptor {
        self.descriptor.clone()
    }

    #[must_use]
    pub fn published_token(&self) -> u64 {
        self.mapping.load_published_token(Ordering::Acquire)
    }

    /// Publishes one complete frame or drops it if both non-current slots are
    /// unavailable.
    ///
    /// # Errors
    ///
    /// Returns [`RingError::NoWritableSlot`] without invalidating the current
    /// publication when no non-current unpinned slot can be acquired.
    pub fn publish(&self, frame: &BgrFrame) -> Result<Publication, RingError> {
        let sequence =
            self.next_sequence
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(if current >= MAX_FRAME_SEQUENCE {
                        0
                    } else {
                        current + 1
                    })
                });
        let sequence = sequence.map_err(|_| RingError::InvalidPublicationToken)?;
        let (slot_index, previous_state) = self.reserve_noncurrent_slot(sequence)?;
        self.mapping.write_slot(slot_index, sequence, frame);
        self.mapping
            .store_slot_state(slot_index, SLOT_PUBLISHED, Ordering::Release);
        self.mapping
            .store_published_token(encode_token(sequence, slot_index)?, Ordering::Release);
        debug_assert!(matches!(previous_state, SLOT_EMPTY | SLOT_PUBLISHED));
        Ok(Publication {
            slot_index,
            frame_sequence: sequence,
            size: frame.size(),
        })
    }

    fn reserve_noncurrent_slot(&self, sequence: u64) -> Result<(usize, u32), RingError> {
        let current = decode_token(self.mapping.load_published_token(Ordering::Acquire))?
            .map(|(slot, _)| slot);
        let start = usize::try_from(sequence % SLOT_COUNT as u64)
            .expect("sequence modulo slot count fits usize");
        for offset in 0..SLOT_COUNT {
            let slot = (start + offset) % SLOT_COUNT;
            if current == Some(slot) {
                continue;
            }
            let state = self.mapping.load_slot_state(slot, Ordering::Acquire);
            if !matches!(state, SLOT_EMPTY | SLOT_PUBLISHED)
                || self.mapping.load_pin_count(slot, Ordering::Acquire) != 0
            {
                continue;
            }
            if self
                .mapping
                .compare_exchange_slot_state(
                    slot,
                    state,
                    SLOT_WRITING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                continue;
            }
            if self.mapping.load_pin_count(slot, Ordering::Acquire) == 0 {
                return Ok((slot, state));
            }
            self.mapping
                .store_slot_state(slot, state, Ordering::Release);
        }
        Err(RingError::NoWritableSlot)
    }

    /// Copies the current complete publication, returning `None` while
    /// publication is invalid.
    ///
    /// # Errors
    ///
    /// Reports corrupt metadata, a single-reader protocol violation, or eight
    /// consecutive publication races.
    pub fn read_published(&self) -> Result<Option<BgrFrame>, RingError> {
        self.read_published_with_hook(|| {})
    }

    fn read_published_with_hook(
        &self,
        mut after_pin: impl FnMut(),
    ) -> Result<Option<BgrFrame>, RingError> {
        for _ in 0..READ_RETRY_LIMIT {
            let token = self.mapping.load_published_token(Ordering::Acquire);
            let Some((slot, sequence)) = decode_token(token)? else {
                return Ok(None);
            };
            let pin = self.pin(slot)?;
            after_pin();
            if self.mapping.load_published_token(Ordering::Acquire) != token
                || self.mapping.load_slot_state(slot, Ordering::Acquire) != SLOT_PUBLISHED
            {
                drop(pin);
                std::thread::yield_now();
                continue;
            }
            let metadata = self.mapping.read_metadata(slot);
            if metadata.frame_sequence != sequence {
                drop(pin);
                std::thread::yield_now();
                continue;
            }
            let frame = validate_and_copy(&self.mapping, slot, metadata)?;
            drop(pin);
            return Ok(Some(frame));
        }
        Err(RingError::RetryExhausted)
    }

    fn pin(&self, slot: usize) -> Result<PinGuard, RingError> {
        self.mapping
            .compare_exchange_pin_count(slot, 0, 1)
            .map_err(|actual| {
                if actual == 1 {
                    RingError::ReaderAlreadyPinned
                } else {
                    RingError::InvalidPinCount
                }
            })?;
        Ok(PinGuard {
            mapping: self.mapping.clone(),
            slot,
            released: false,
        })
    }

    /// Pins the current slot for stress tests and cross-component diagnostics.
    /// The guard can copy the same historical frame even after a newer slot is
    /// published.
    ///
    /// # Errors
    ///
    /// Returns a protocol error if another reader is already pinned.
    pub fn pin_current_for_diagnostics(&self) -> Result<Option<DebugPinnedFrame>, RingError> {
        for _ in 0..READ_RETRY_LIMIT {
            let token = self.mapping.load_published_token(Ordering::Acquire);
            let Some((slot, sequence)) = decode_token(token)? else {
                return Ok(None);
            };
            let pin = self.pin(slot)?;
            if self.mapping.load_published_token(Ordering::Acquire) == token
                && self.mapping.load_slot_state(slot, Ordering::Acquire) == SLOT_PUBLISHED
                && self.mapping.load_frame_sequence(slot, Ordering::Acquire) == sequence
            {
                return Ok(Some(DebugPinnedFrame { pin, sequence }));
            }
            drop(pin);
            std::thread::yield_now();
        }
        Err(RingError::RetryExhausted)
    }

    /// Recovers the sole worker's abandoned pin only after OS process reap and
    /// before a replacement reader exists.
    ///
    /// # Errors
    ///
    /// Rejects recovery until both single-reader safety preconditions hold.
    pub fn recover_reader_pins(
        &self,
        worker_process_reaped: bool,
        replacement_reader_absent: bool,
    ) -> Result<usize, RingError> {
        if !worker_process_reaped || !replacement_reader_absent {
            return Err(RingError::UnsafePinRecovery);
        }
        let mut recovered = 0;
        for slot in 0..SLOT_COUNT {
            match self.mapping.load_pin_count(slot, Ordering::Acquire) {
                0 => {}
                1 => {
                    self.mapping.store_pin_count(slot, 0, Ordering::Release);
                    recovered += 1;
                }
                _ => return Err(RingError::InvalidPinCount),
            }
        }
        Ok(recovered)
    }

    /// Invalidates publication only after the capture writer has stopped. Any
    /// abandoned non-current writing slot is reset to empty first.
    ///
    /// # Errors
    ///
    /// Returns without changing the mapping if writer termination is unproven.
    pub fn stop_publication(&self, writer_stopped: bool) -> Result<usize, RingError> {
        if !writer_stopped {
            return Err(RingError::WriterStillRunning);
        }
        let current = decode_token(self.mapping.load_published_token(Ordering::Acquire))?
            .map(|(slot, _)| slot);
        let mut recovered = 0;
        for slot in 0..SLOT_COUNT {
            if self.mapping.load_slot_state(slot, Ordering::Acquire) == SLOT_WRITING {
                if current == Some(slot) {
                    return Err(RingError::InvalidSlotState);
                }
                self.mapping
                    .store_slot_state(slot, SLOT_EMPTY, Ordering::Release);
                recovered += 1;
            }
        }
        self.mapping
            .store_published_token(INVALID_PUBLISHED_TOKEN, Ordering::Release);
        Ok(recovered)
    }
}

/// Per-worker reader that owns the last complete private frame fallback.
#[derive(Debug)]
pub struct RingReader {
    ring: SharedFrameRing,
    last_complete_frame: Mutex<Option<BgrFrame>>,
}

impl RingReader {
    #[must_use]
    pub fn new(ring: SharedFrameRing) -> Self {
        Self {
            ring,
            last_complete_frame: Mutex::new(None),
        }
    }

    /// Returns an independent mutable frame. Invalid publication yields zero;
    /// eight races yield a same-sized last complete frame or zero.
    ///
    /// # Errors
    ///
    /// Propagates mapping corruption and single-reader protocol violations.
    pub fn read(&self, resolution: CaptureResolution) -> Result<BgrFrame, RingError> {
        self.read_with(resolution, || {})
    }

    fn read_with(
        &self,
        resolution: CaptureResolution,
        hook: impl FnMut(),
    ) -> Result<BgrFrame, RingError> {
        let expected = resolution.size();
        match self.ring.read_published_with_hook(hook) {
            Ok(Some(frame)) if frame.size() == expected => {
                *self.lock_history() = Some(frame.clone());
                Ok(frame)
            }
            Ok(Some(_) | None) => Ok(BgrFrame::zero(resolution)),
            Err(RingError::RetryExhausted) => Ok(self
                .lock_history()
                .as_ref()
                .filter(|frame| frame.size() == expected)
                .cloned()
                .unwrap_or_else(|| BgrFrame::zero(resolution))),
            Err(error) => Err(error),
        }
    }

    fn lock_history(&self) -> MutexGuard<'_, Option<BgrFrame>> {
        self.last_complete_frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Diagnostic pin guard used to prove pinned-slot immutability.
#[derive(Debug)]
pub struct DebugPinnedFrame {
    pin: PinGuard,
    sequence: u64,
}

impl DebugPinnedFrame {
    #[must_use]
    pub const fn slot_index(&self) -> usize {
        self.pin.slot
    }

    /// Copies the pinned historical frame without consulting the newer current
    /// token.
    ///
    /// # Errors
    ///
    /// Reports any mutation or corrupt metadata observed while pinned.
    pub fn copy_frame(&self) -> Result<BgrFrame, RingError> {
        let metadata = self.pin.mapping.read_metadata(self.pin.slot);
        if metadata.frame_sequence != self.sequence
            || self
                .pin
                .mapping
                .load_slot_state(self.pin.slot, Ordering::Acquire)
                != SLOT_PUBLISHED
        {
            return Err(RingError::CorruptMetadata);
        }
        validate_and_copy(&self.pin.mapping, self.pin.slot, metadata)
    }

    /// Simulates a process crash by intentionally abandoning the shared pin
    /// while still releasing this process's mapping handle.
    #[doc(hidden)]
    pub fn abandon_for_crash_simulation(mut self) {
        self.pin.released = true;
    }
}

#[derive(Debug)]
struct PinGuard {
    mapping: Arc<mapping::MappedRing>,
    slot: usize,
    released: bool,
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        if self
            .mapping
            .compare_exchange_pin_count(self.slot, 1, 0)
            .is_err()
        {
            tracing::error!(
                diagnostic_id = "CAMERA_SHM_PIN_RELEASE_INVALID",
                slot = self.slot,
                "camera shared-memory reader pin could not be released"
            );
        }
    }
}

fn validate_and_copy(
    mapping: &mapping::MappedRing,
    slot: usize,
    metadata: mapping::SlotMetadata,
) -> Result<BgrFrame, RingError> {
    if metadata.dtype != DTYPE_UINT8
        || metadata.shape[2] != 3
        || metadata.shape[3] != 0
        || metadata.strides[2] != 1
        || metadata.strides[3] != 0
    {
        return Err(RingError::CorruptMetadata);
    }
    let width = u32::try_from(metadata.shape[1]).map_err(|_| RingError::CorruptMetadata)?;
    let height = u32::try_from(metadata.shape[0]).map_err(|_| RingError::CorruptMetadata)?;
    let size = FrameSize::new(width, height).map_err(|_| RingError::CorruptMetadata)?;
    let byte_length =
        usize::try_from(metadata.byte_length).map_err(|_| RingError::CorruptMetadata)?;
    if byte_length != size.byte_len()
        || metadata.strides[0] != u64::from(width) * 3
        || metadata.strides[1] != 3
    {
        return Err(RingError::CorruptMetadata);
    }
    let mut pixels = vec![0; byte_length];
    mapping.copy_payload(slot, &mut pixels);
    BgrFrame::new(width, height, pixels).map_err(Into::into)
}

fn encode_token(sequence: u64, slot: usize) -> Result<u64, RingError> {
    if sequence > MAX_FRAME_SEQUENCE || slot >= SLOT_COUNT {
        return Err(RingError::InvalidPublicationToken);
    }
    Ok((sequence << 2) | u64::try_from(slot).expect("slot index fits u64"))
}

fn decode_token(token: u64) -> Result<Option<(usize, u64)>, RingError> {
    if token == INVALID_PUBLISHED_TOKEN {
        return Ok(None);
    }
    let slot = usize::try_from(token & 3).expect("two-bit slot fits usize");
    let sequence = token >> 2;
    if slot >= SLOT_COUNT || sequence > MAX_FRAME_SEQUENCE {
        return Err(RingError::InvalidPublicationToken);
    }
    Ok(Some((slot, sequence)))
}

/// Shared-frame protocol error with no mapping name or OS diagnostic text.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RingError {
    #[error("camera shared-memory mapping operation failed")]
    MappingFailed,
    #[error("camera shared-memory descriptor is incompatible")]
    InvalidDescriptor,
    #[error("no non-current unpinned camera slot is writable")]
    NoWritableSlot,
    #[error("camera publication token is invalid")]
    InvalidPublicationToken,
    #[error("camera slot state is invalid")]
    InvalidSlotState,
    #[error("the sole camera reader already has a pinned slot")]
    ReaderAlreadyPinned,
    #[error("camera reader pin count violates the single-reader protocol")]
    InvalidPinCount,
    #[error("camera slot metadata is corrupt")]
    CorruptMetadata,
    #[error("camera reader lost eight consecutive publication races")]
    RetryExhausted,
    #[error("camera worker pin recovery preconditions are not satisfied")]
    UnsafePinRecovery,
    #[error("camera writer termination has not been confirmed")]
    WriterStillRunning,
    #[error(transparent)]
    Frame(#[from] FrameError),
}

#[allow(unsafe_code)]
mod mapping {
    use std::mem::{align_of, size_of};
    use std::ptr;
    use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

    use shared_memory::{Shmem, ShmemConf};

    use super::{
        BgrFrame, DTYPE_UINT8, INVALID_PUBLISHED_TOKEN, RingError, SLOT_BYTE_SIZE, SLOT_COUNT,
        SLOT_EMPTY,
    };

    #[repr(C)]
    struct SharedHeader {
        published_token: AtomicU64,
    }

    #[repr(C)]
    struct SlotHeader {
        frame_sequence: AtomicU64,
        state: AtomicU32,
        reader_pin_count: AtomicI32,
        byte_length: u64,
        dtype: [u8; 8],
        shape: [u64; 4],
        strides: [u64; 4],
    }

    const SHARED_HEADER_SIZE: usize = size_of::<SharedHeader>();
    const SLOT_HEADER_SIZE: usize = size_of::<SlotHeader>();
    const SLOT_STRIDE: usize = SLOT_HEADER_SIZE + SLOT_BYTE_SIZE;
    pub(super) const TOTAL_SIZE: usize = SHARED_HEADER_SIZE + SLOT_COUNT * SLOT_STRIDE;

    const _: () = assert!(align_of::<SharedHeader>() <= 8);
    const _: () = assert!(align_of::<SlotHeader>() <= 8);
    const _: () = assert!(SHARED_HEADER_SIZE.is_multiple_of(8));
    const _: () = assert!(SLOT_STRIDE.is_multiple_of(8));

    #[derive(Clone, Copy)]
    pub(super) struct SlotMetadata {
        pub frame_sequence: u64,
        pub byte_length: u64,
        pub dtype: [u8; 8],
        pub shape: [u64; 4],
        pub strides: [u64; 4],
    }

    pub(super) struct MappedRing {
        mapping: Shmem,
    }

    impl std::fmt::Debug for MappedRing {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("MappedRing")
                .field("owner", &self.mapping.is_owner())
                .field("length", &self.mapping.len())
                .finish_non_exhaustive()
        }
    }

    // SAFETY: `Shmem` is a process mapping handle whose raw bytes are accessed
    // only through the atomically coordinated fixed ring methods below. Drop is
    // still uniquely owned by `MappedRing`, and the mapping outlives every raw
    // access through the enclosing `Arc`.
    unsafe impl Send for MappedRing {}
    // SAFETY: Shared access performs only atomic header operations or payload
    // copies protected by the writer-state/reader-pin protocol. No Rust
    // references to payload bytes escape this type.
    unsafe impl Sync for MappedRing {}

    impl MappedRing {
        pub(super) fn create() -> Result<Self, RingError> {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random).map_err(|_| RingError::MappingFailed)?;
            let name = format!(
                "pokecon-camera-{}-{}",
                std::process::id(),
                hex::encode(random)
            );
            let mapping = ShmemConf::new()
                .size(TOTAL_SIZE)
                .os_id(name)
                .create()
                .map_err(|_| RingError::MappingFailed)?;
            let ring = Self { mapping };
            ring.initialize();
            Ok(ring)
        }

        pub(super) fn open(os_id: &str, size: usize) -> Result<Self, RingError> {
            if size != TOTAL_SIZE || os_id.is_empty() {
                return Err(RingError::InvalidDescriptor);
            }
            let mapping = ShmemConf::new()
                .size(size)
                .os_id(os_id)
                .open()
                .map_err(|_| RingError::MappingFailed)?;
            if mapping.len() != TOTAL_SIZE {
                return Err(RingError::InvalidDescriptor);
            }
            Ok(Self { mapping })
        }

        pub(super) fn os_id(&self) -> &str {
            self.mapping.get_os_id()
        }

        fn initialize(&self) {
            // SAFETY: This mapping was created exclusively above, has exactly
            // `TOTAL_SIZE` bytes, and no descriptor has been published yet.
            unsafe {
                ptr::write_bytes(self.mapping.as_ptr(), 0, TOTAL_SIZE);
                ptr::write(
                    self.shared_header_ptr(),
                    SharedHeader {
                        published_token: AtomicU64::new(INVALID_PUBLISHED_TOKEN),
                    },
                );
                for slot in 0..SLOT_COUNT {
                    ptr::write(
                        self.slot_header_ptr(slot),
                        SlotHeader {
                            frame_sequence: AtomicU64::new(0),
                            state: AtomicU32::new(SLOT_EMPTY),
                            reader_pin_count: AtomicI32::new(0),
                            byte_length: 0,
                            dtype: [0; 8],
                            shape: [0; 4],
                            strides: [0; 4],
                        },
                    );
                }
            }
        }

        pub(super) fn load_published_token(&self, ordering: Ordering) -> u64 {
            self.shared_header().published_token.load(ordering)
        }

        pub(super) fn store_published_token(&self, value: u64, ordering: Ordering) {
            self.shared_header().published_token.store(value, ordering);
        }

        pub(super) fn load_slot_state(&self, slot: usize, ordering: Ordering) -> u32 {
            self.slot_header(slot).state.load(ordering)
        }

        pub(super) fn store_slot_state(&self, slot: usize, value: u32, ordering: Ordering) {
            self.slot_header(slot).state.store(value, ordering);
        }

        pub(super) fn compare_exchange_slot_state(
            &self,
            slot: usize,
            current: u32,
            new: u32,
            success: Ordering,
            failure: Ordering,
        ) -> Result<u32, u32> {
            self.slot_header(slot)
                .state
                .compare_exchange(current, new, success, failure)
        }

        pub(super) fn load_pin_count(&self, slot: usize, ordering: Ordering) -> i32 {
            self.slot_header(slot).reader_pin_count.load(ordering)
        }

        pub(super) fn store_pin_count(&self, slot: usize, value: i32, ordering: Ordering) {
            self.slot_header(slot)
                .reader_pin_count
                .store(value, ordering);
        }

        pub(super) fn compare_exchange_pin_count(
            &self,
            slot: usize,
            current: i32,
            new: i32,
        ) -> Result<i32, i32> {
            self.slot_header(slot).reader_pin_count.compare_exchange(
                current,
                new,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
        }

        pub(super) fn load_frame_sequence(&self, slot: usize, ordering: Ordering) -> u64 {
            self.slot_header(slot).frame_sequence.load(ordering)
        }

        pub(super) fn write_slot(&self, slot: usize, sequence: u64, frame: &BgrFrame) {
            let size = frame.size();
            let header = self.slot_header_ptr(slot);
            // SAFETY: The caller atomically owns this non-current slot in state
            // WRITING and observed pin count zero. Offsets are fixed, aligned,
            // and the validated frame cannot exceed `SLOT_BYTE_SIZE`.
            unsafe {
                (*header).frame_sequence.store(sequence, Ordering::Relaxed);
                ptr::write(
                    ptr::addr_of_mut!((*header).byte_length),
                    u64::try_from(frame.pixels().len()).expect("frame bytes fit u64"),
                );
                ptr::write(ptr::addr_of_mut!((*header).dtype), DTYPE_UINT8);
                ptr::write(
                    ptr::addr_of_mut!((*header).shape),
                    [u64::from(size.height()), u64::from(size.width()), 3, 0],
                );
                ptr::write(
                    ptr::addr_of_mut!((*header).strides),
                    [u64::from(size.width()) * 3, 3, 1, 0],
                );
                ptr::copy_nonoverlapping(
                    frame.pixels().as_ptr(),
                    self.payload_ptr(slot),
                    frame.pixels().len(),
                );
            }
        }

        pub(super) fn read_metadata(&self, slot: usize) -> SlotMetadata {
            let header = self.slot_header_ptr(slot);
            // SAFETY: The reader holds the slot pin, verified a stable token,
            // and acquired state PUBLISHED before reading immutable metadata.
            unsafe {
                SlotMetadata {
                    frame_sequence: (*header).frame_sequence.load(Ordering::Relaxed),
                    byte_length: ptr::read(ptr::addr_of!((*header).byte_length)),
                    dtype: ptr::read(ptr::addr_of!((*header).dtype)),
                    shape: ptr::read(ptr::addr_of!((*header).shape)),
                    strides: ptr::read(ptr::addr_of!((*header).strides)),
                }
            }
        }

        pub(super) fn copy_payload(&self, slot: usize, destination: &mut [u8]) {
            debug_assert!(destination.len() <= SLOT_BYTE_SIZE);
            // SAFETY: The destination is a unique owned vector slice, the
            // source range is within the fixed slot, and the caller holds the
            // verified reader pin throughout this copy.
            unsafe {
                ptr::copy_nonoverlapping(
                    self.payload_ptr(slot),
                    destination.as_mut_ptr(),
                    destination.len(),
                );
            }
        }

        fn shared_header(&self) -> &SharedHeader {
            // SAFETY: Creator initialization constructs the aligned header
            // before publishing the descriptor; opener validates fixed size.
            unsafe { &*self.shared_header_ptr() }
        }

        fn slot_header(&self, slot: usize) -> &SlotHeader {
            assert!(slot < SLOT_COUNT, "validated slot index");
            // SAFETY: The fixed layout initializes every aligned slot header,
            // and the mapping outlives the returned internal reference.
            unsafe { &*self.slot_header_ptr(slot) }
        }

        fn shared_header_ptr(&self) -> *mut SharedHeader {
            self.mapping.as_ptr().cast()
        }

        fn slot_header_ptr(&self, slot: usize) -> *mut SlotHeader {
            debug_assert!(slot < SLOT_COUNT);
            self.mapping
                .as_ptr()
                .wrapping_add(SHARED_HEADER_SIZE + slot * SLOT_STRIDE)
                .cast()
        }

        fn payload_ptr(&self, slot: usize) -> *mut u8 {
            self.slot_header_ptr(slot)
                .cast::<u8>()
                .wrapping_add(SLOT_HEADER_SIZE)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{INVALID_PUBLISHED_TOKEN, RingError, RingReader, SLOT_PUBLISHED, SharedFrameRing};
    use crate::camera::frame::{BgrFrame, CaptureResolution};

    fn frame(resolution: CaptureResolution, value: u8) -> BgrFrame {
        BgrFrame::solid(
            resolution,
            [value, value.wrapping_add(1), value.wrapping_add(2)],
        )
    }

    #[test]
    fn descriptor_opens_same_mapping_and_invalid_publication_returns_zero() {
        let owner = SharedFrameRing::create(CaptureResolution::R640x360).unwrap();
        let reader_ring = SharedFrameRing::open(owner.descriptor()).unwrap();
        let reader = RingReader::new(reader_ring);
        assert_eq!(
            reader.read(CaptureResolution::R640x360).unwrap(),
            BgrFrame::zero(CaptureResolution::R640x360)
        );
        owner
            .publish(&frame(CaptureResolution::R640x360, 7))
            .unwrap();
        let mut first = reader.read(CaptureResolution::R640x360).unwrap();
        first.pixels_mut()[0] = 255;
        assert_eq!(
            reader.read(CaptureResolution::R640x360).unwrap().pixels()[0],
            7
        );
    }

    #[test]
    fn pinned_slot_data_and_metadata_survive_rotation_and_resolution_change() {
        let ring = SharedFrameRing::create(CaptureResolution::R640x360).unwrap();
        let original = frame(CaptureResolution::R640x360, 10);
        let publication = ring.publish(&original).unwrap();
        let pinned = ring
            .pin_current_for_diagnostics()
            .unwrap()
            .expect("published frame is pinnable");
        assert_eq!(pinned.slot_index(), publication.slot_index);
        ring.publish(&frame(CaptureResolution::R1280x720, 20))
            .unwrap();
        ring.publish(&frame(CaptureResolution::R1280x720, 30))
            .unwrap();
        ring.publish(&frame(CaptureResolution::R1280x720, 40))
            .unwrap();
        assert_eq!(pinned.copy_frame().unwrap(), original);
        assert_ne!(
            ring.read_published().unwrap().unwrap().size(),
            original.size()
        );
    }

    #[test]
    fn eight_races_return_same_shape_history_and_never_partial_data() {
        let ring = SharedFrameRing::create(CaptureResolution::R640x360).unwrap();
        ring.publish(&frame(CaptureResolution::R640x360, 1))
            .unwrap();
        let reader = RingReader::new(ring.clone());
        let history = reader.read(CaptureResolution::R640x360).unwrap();
        let mut next = 10_u8;
        let fallback = reader
            .read_with(CaptureResolution::R640x360, || {
                ring.publish(&frame(CaptureResolution::R640x360, next))
                    .unwrap();
                next = next.wrapping_add(1);
            })
            .unwrap();
        assert_eq!(fallback, history);
    }

    #[test]
    fn crash_pin_recovery_and_writer_stop_require_proven_preconditions() {
        let ring = SharedFrameRing::create(CaptureResolution::R640x360).unwrap();
        ring.publish(&frame(CaptureResolution::R640x360, 5))
            .unwrap();
        ring.pin_current_for_diagnostics()
            .unwrap()
            .unwrap()
            .abandon_for_crash_simulation();
        assert_eq!(
            ring.recover_reader_pins(false, true),
            Err(RingError::UnsafePinRecovery)
        );
        assert_eq!(ring.recover_reader_pins(true, true).unwrap(), 1);

        let (reserved, _) = ring.reserve_noncurrent_slot(42).unwrap();
        assert_eq!(
            ring.stop_publication(false),
            Err(RingError::WriterStillRunning)
        );
        assert_eq!(
            ring.mapping.load_slot_state(reserved, Ordering::Acquire),
            super::SLOT_WRITING
        );
        assert_eq!(ring.stop_publication(true).unwrap(), 1);
        assert_eq!(ring.published_token(), INVALID_PUBLISHED_TOKEN);
        assert_eq!(
            ring.mapping.load_slot_state(reserved, Ordering::Acquire),
            super::SLOT_EMPTY
        );
        let current_states = (0..super::SLOT_COUNT)
            .filter(|slot| ring.mapping.load_slot_state(*slot, Ordering::Acquire) == SLOT_PUBLISHED)
            .count();
        assert!(current_states >= 1);
    }

    #[test]
    fn concurrent_rotation_never_exposes_partial_payload_or_metadata() {
        let ring = Arc::new(SharedFrameRing::create(CaptureResolution::R640x360).unwrap());
        ring.publish(&frame(CaptureResolution::R640x360, 1))
            .unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let writer_ring = ring.clone();
        let writer_finished = finished.clone();
        let writer = std::thread::spawn(move || {
            for sequence in 0..40_u8 {
                let resolution = if sequence.is_multiple_of(2) {
                    CaptureResolution::R640x360
                } else {
                    CaptureResolution::R1280x720
                };
                loop {
                    match writer_ring.publish(&frame(resolution, sequence)) {
                        Ok(_) => break,
                        Err(RingError::NoWritableSlot) => std::thread::yield_now(),
                        Err(error) => panic!("unexpected publication failure: {error}"),
                    }
                }
            }
            writer_finished.store(true, Ordering::Release);
        });
        while !finished.load(Ordering::Acquire) {
            if let Some(observed) = ring.read_published().unwrap() {
                let first = observed.pixels()[0];
                assert!(observed.pixels().chunks_exact(3).all(|pixel| {
                    pixel == [first, first.wrapping_add(1), first.wrapping_add(2)]
                }));
                assert!(matches!(
                    (observed.size().width(), observed.size().height()),
                    (640, 360) | (1280, 720)
                ));
            }
        }
        writer.join().unwrap();
    }
}
