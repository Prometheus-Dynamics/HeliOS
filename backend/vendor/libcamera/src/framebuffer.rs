use std::{
    io,
    marker::PhantomData,
    os::fd::{IntoRawFd, OwnedFd},
    ptr::NonNull,
};

use libcamera_sys::*;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{fence::Fence, utils::Immutable};

#[derive(Debug, Clone, Copy, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum FrameMetadataStatus {
    Success = libcamera_frame_metadata_status::LIBCAMERA_FRAME_METADATA_STATUS_SUCCESS,
    Error = libcamera_frame_metadata_status::LIBCAMERA_FRAME_METADATA_STATUS_ERROR,
    Cancelled = libcamera_frame_metadata_status::LIBCAMERA_FRAME_METADATA_STATUS_CANCELLED,
    Startup = libcamera_frame_metadata_status::LIBCAMERA_FRAME_METADATA_STATUS_STARTUP,
}

pub type FrameMetadataPlane = libcamera_frame_metadata_plane_t;

pub struct FrameMetadataPlanes {
    pub(crate) ptr: NonNull<libcamera_frame_metadata_planes_t>,
}

impl FrameMetadataPlanes {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_frame_metadata_planes_t>) -> Self {
        Self { ptr }
    }

    /// Number of planes within framebuffer metadata.
    ///
    /// Should be consistent with other planes within framebuffer.
    pub fn len(&self) -> usize {
        unsafe { libcamera_frame_metadata_planes_size(self.ptr.as_ptr()) as _ }
    }

    /// Returns `true` if there are no planes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns framebuffer plane metadata at a given index.
    ///
    /// Return None if given index is out of range of available planes.
    pub fn get(&self, index: usize) -> Option<FrameMetadataPlane> {
        if index >= self.len() {
            None
        } else {
            Some(unsafe { libcamera_frame_metadata_planes_at(self.ptr.as_ptr(), index as _).read() })
        }
    }
}

impl core::fmt::Debug for FrameMetadataPlanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for plane in self.into_iter() {
            list.entry(&plane);
        }
        list.finish()
    }
}

impl Drop for FrameMetadataPlanes {
    fn drop(&mut self) {
        unsafe { libcamera_frame_metadata_planes_destroy(self.ptr.as_ptr()) }
    }
}

impl<'d> IntoIterator for &'d FrameMetadataPlanes {
    type Item = FrameMetadataPlane;

    type IntoIter = FrameMetadataPlanesIterator<'d>;

    fn into_iter(self) -> Self::IntoIter {
        FrameMetadataPlanesIterator { planes: self, index: 0 }
    }
}

pub struct FrameMetadataPlanesIterator<'d> {
    planes: &'d FrameMetadataPlanes,
    index: usize,
}

impl Iterator for FrameMetadataPlanesIterator<'_> {
    type Item = FrameMetadataPlane;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(plane) = self.planes.get(self.index) {
            self.index += 1;
            Some(plane)
        } else {
            None
        }
    }
}

pub struct FrameMetadataRef<'d> {
    pub(crate) ptr: NonNull<libcamera_frame_metadata_t>,
    _phantom: PhantomData<&'d ()>,
}

impl FrameMetadataRef<'_> {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_frame_metadata_t>) -> Self {
        Self {
            ptr,
            _phantom: Default::default(),
        }
    }

    pub fn status(&self) -> FrameMetadataStatus {
        FrameMetadataStatus::try_from(unsafe { libcamera_frame_metadata_status(self.ptr.as_ptr()) }).unwrap()
    }

    pub fn sequence(&self) -> u32 {
        unsafe { libcamera_frame_metadata_sequence(self.ptr.as_ptr()) }
    }

    pub fn timestamp(&self) -> u64 {
        unsafe { libcamera_frame_metadata_timestamp(self.ptr.as_ptr()) }
    }

    pub fn planes(&self) -> FrameMetadataPlanes {
        unsafe {
            FrameMetadataPlanes::from_ptr(NonNull::new(libcamera_frame_metadata_planes(self.ptr.as_ptr())).unwrap())
        }
    }
}

impl core::fmt::Debug for FrameMetadataRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameMetadataRef")
            .field("status", &self.status())
            .field("sequence", &self.sequence())
            .field("timestamp", &self.timestamp())
            .field("planes", &self.planes())
            .finish()
    }
}

pub struct FrameBufferPlaneRef<'d> {
    pub(crate) ptr: NonNull<libcamera_framebuffer_plane_t>,
    _phantom: PhantomData<&'d ()>,
}

impl FrameBufferPlaneRef<'_> {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_framebuffer_plane_t>) -> Self {
        Self {
            ptr,
            _phantom: Default::default(),
        }
    }

    /// File descriptor to the framebuffer plane data.
    ///
    /// Multiple planes may point to the same file descriptor at different offsets.
    pub fn fd(&self) -> i32 {
        unsafe { libcamera_framebuffer_plane_fd(self.ptr.as_ptr()) }
    }

    /// Offset of data within the file descriptor.
    pub fn offset(&self) -> Option<usize> {
        if unsafe { libcamera_framebuffer_plane_offset_valid(self.ptr.as_ptr()) } {
            Some(unsafe { libcamera_framebuffer_plane_offset(self.ptr.as_ptr()) as _ })
        } else {
            None
        }
    }

    /// Data length of the plane in bytes
    pub fn len(&self) -> usize {
        unsafe { libcamera_framebuffer_plane_length(self.ptr.as_ptr()) as _ }
    }

    /// Returns `true` if plane has no data
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl core::fmt::Debug for FrameBufferPlaneRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameBufferPlaneRef")
            .field("fd", &self.fd())
            .field("offset", &self.offset())
            .field("len", &self.len())
            .finish()
    }
}

pub struct FrameBufferPlanesRef<'d> {
    pub(crate) ptr: NonNull<libcamera_framebuffer_planes_t>,
    _phantom: PhantomData<&'d ()>,
}

impl FrameBufferPlanesRef<'_> {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_framebuffer_planes_t>) -> Self {
        Self {
            ptr,
            _phantom: Default::default(),
        }
    }

    /// Number of planes within framebuffer
    pub fn len(&self) -> usize {
        unsafe { libcamera_framebuffer_planes_size(self.ptr.as_ptr()) as _ }
    }

    /// Returns `true` if framebuffer has no planes
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns framebuffer plane at a given index
    pub fn get(&self, index: usize) -> Option<Immutable<FrameBufferPlaneRef<'_>>> {
        if index >= self.len() {
            None
        } else {
            Some(Immutable(unsafe {
                FrameBufferPlaneRef::from_ptr(
                    NonNull::new(libcamera_framebuffer_planes_at(self.ptr.as_ptr(), index as _)).unwrap(),
                )
            }))
        }
    }
}

impl core::fmt::Debug for FrameBufferPlanesRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for plane in self.into_iter() {
            list.entry(&plane);
        }
        list.finish()
    }
}

impl Drop for FrameBufferPlanesRef<'_> {
    fn drop(&mut self) {
        unsafe { libcamera_framebuffer_planes_destroy(self.ptr.as_ptr()) }
    }
}

impl<'d> IntoIterator for &'d FrameBufferPlanesRef<'d> {
    type Item = Immutable<FrameBufferPlaneRef<'d>>;

    type IntoIter = FrameBufferPlanesRefIterator<'d>;

    fn into_iter(self) -> Self::IntoIter {
        FrameBufferPlanesRefIterator { planes: self, index: 0 }
    }
}

pub struct FrameBufferPlanesRefIterator<'d> {
    planes: &'d FrameBufferPlanesRef<'d>,
    index: usize,
}

impl<'d> Iterator for FrameBufferPlanesRefIterator<'d> {
    type Item = Immutable<FrameBufferPlaneRef<'d>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(plane) = self.planes.get(self.index) {
            self.index += 1;
            Some(plane)
        } else {
            None
        }
    }
}

pub trait AsFrameBuffer: Send {
    /// Returns raw framebuffer used by libcamera.
    ///
    /// It is expected that metadata status field is initialized with u32::MAX on a new buffer, which indicates that
    /// metadata is not yet available. This "hackfix" prevents read of uninitialized data in [Self::metadata()].
    ///
    /// # Safety
    ///
    /// This function must return a valid instance of `libcamera::FrameBuffer`.
    unsafe fn ptr(&self) -> NonNull<libcamera_framebuffer_t>;

    /// Returns framebuffer metadata information.
    ///
    /// Only available after associated [Request](crate::request::Request) has completed.
    fn metadata(&self) -> Option<Immutable<FrameMetadataRef<'_>>> {
        let ptr = NonNull::new(unsafe { libcamera_framebuffer_metadata(self.ptr().as_ptr()) }.cast_mut()).unwrap();
        if unsafe { libcamera_frame_metadata_status(ptr.as_ptr()) } != u32::MAX {
            Some(unsafe { Immutable(FrameMetadataRef::from_ptr(ptr)) })
        } else {
            None
        }
    }

    /// Provides access to framebuffer data by exposing file descriptors, offsets and lengths of the planes.
    fn planes(&self) -> Immutable<FrameBufferPlanesRef<'_>> {
        unsafe {
            Immutable(FrameBufferPlanesRef::from_ptr(
                NonNull::new(libcamera_framebuffer_planes(self.ptr().as_ptr())).unwrap(),
            ))
        }
    }

    /// User cookie associated with the buffer.
    fn cookie(&self) -> u64 {
        unsafe { libcamera_framebuffer_cookie(self.ptr().as_ptr()) }
    }

    /// Set user cookie associated with the buffer.
    fn set_cookie(&self, cookie: u64) {
        unsafe { libcamera_framebuffer_set_cookie(self.ptr().as_ptr(), cookie) }
    }

    /// Releases the acquire fence associated with this framebuffer, if any.
    ///
    /// Ownership of the fence is transferred to the caller.
    fn release_fence(&self) -> Option<Fence> {
        unsafe { Fence::from_ptr(libcamera_framebuffer_release_fence_handle(self.ptr().as_ptr())) }
    }

    /// Returns a non-owning view of the Request owning this framebuffer, if any.
    fn request(&self) -> Option<crate::request::RequestRef<'_>> {
        let ptr = unsafe { libcamera_framebuffer_request(self.ptr().as_ptr()) };
        NonNull::new(ptr).map(|p| unsafe { crate::request::RequestRef::from_ptr(p) })
    }
}

/// Description of a framebuffer plane for importing buffers.
pub struct FrameBufferPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub length: u32,
}

/// FrameBuffer created from user-provided DMABUFs.
pub struct OwnedFrameBuffer {
    ptr: NonNull<libcamera_framebuffer_t>,
}

unsafe impl Send for OwnedFrameBuffer {}

impl OwnedFrameBuffer {
    /// Create a framebuffer from a set of planes. Ownership of the fds is transferred.
    pub fn new(planes: Vec<FrameBufferPlane>, cookie: Option<u64>) -> io::Result<Self> {
        if planes.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no planes provided"));
        }

        let mut infos: Vec<libcamera_framebuffer_plane_info> = Vec::with_capacity(planes.len());
        let mut raw_fds = Vec::with_capacity(planes.len());

        for plane in planes {
            if plane.offset == u32::MAX || plane.length == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "plane offset/length must be valid and non-zero",
                ));
            }
            let fd = plane.fd.into_raw_fd();
            raw_fds.push(fd);
            infos.push(libcamera_framebuffer_plane_info {
                fd,
                offset: plane.offset,
                length: plane.length,
            });
        }

        let ptr = unsafe { libcamera_framebuffer_create(infos.as_ptr(), infos.len(), cookie.unwrap_or(0)) };

        if let Some(ptr) = NonNull::new(ptr) {
            // Initialize metadata status sentinel to avoid uninitialized reads.
            unsafe {
                libcamera_framebuffer_metadata(ptr.as_ptr())
                    .cast_mut()
                    .cast::<u32>()
                    .write(u32::MAX)
            };
            Ok(Self { ptr })
        } else {
            for fd in raw_fds {
                unsafe { libc::close(fd) };
            }
            Err(io::Error::last_os_error())
        }
    }
}

impl AsFrameBuffer for OwnedFrameBuffer {
    unsafe fn ptr(&self) -> NonNull<libcamera_framebuffer_t> {
        self.ptr
    }
}

impl OwnedFrameBuffer {
    /// Returns true if planes are contiguous within a single FD and ordered without gaps.
    pub fn is_contiguous(&self) -> bool {
        let planes = self.planes();
        if planes.is_empty() {
            return false;
        }
        // Gather (fd, offset, length)
        let mut entries: Vec<(i32, usize, usize)> = planes
            .into_iter()
            .filter_map(|p| p.offset().map(|off| (p.fd(), off, p.len())))
            .collect();
        // Must all share the same fd
        if entries
            .iter()
            .map(|e| e.0)
            .collect::<std::collections::HashSet<_>>()
            .len()
            != 1
        {
            return false;
        }
        entries.sort_by_key(|e| e.1);
        // Check contiguous offsets
        let mut expected = entries[0].1;
        for (_, off, len) in entries {
            if off != expected {
                return false;
            }
            expected = off + len;
        }
        true
    }
}

impl core::fmt::Debug for OwnedFrameBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedFrameBuffer")
            .field("cookie", &self.cookie())
            .field("planes", &self.planes())
            .finish()
    }
}

impl Drop for OwnedFrameBuffer {
    fn drop(&mut self) {
        unsafe { libcamera_framebuffer_destroy(self.ptr.as_ptr()) }
    }
}
