use std::{
    ffi::{CStr, CString},
    io,
    marker::PhantomData,
    ptr::NonNull,
    sync::{mpsc, Arc, Mutex},
};

use libcamera_sys::*;

use crate::{camera::Camera, logging::LoggingLevel, utils::handle_result};

struct ManagerCallbacks {
    added: Option<Box<dyn FnMut(Camera<'static>) + Send>>,
    removed: Option<Box<dyn FnMut(Camera<'static>) + Send>>,
    hotplug_tx: Option<mpsc::Sender<HotplugEvent>>,
}

/// Hotplug event propagated via channel helper.
#[derive(Debug, Clone)]
pub enum HotplugEvent {
    Added(String),
    Removed(String),
}

/// Camera manager used to enumerate available cameras in the system.
pub struct CameraManager {
    ptr: NonNull<libcamera_camera_manager_t>,
    callbacks: Box<ManagerCallbacks>,
    added_handle: *mut libcamera_callback_handle_t,
    removed_handle: *mut libcamera_callback_handle_t,
    started: bool,
    tracker: Arc<CameraTracker>,
}

impl CameraManager {
    /// Initializes `libcamera`, starts the manager and creates [Self].
    pub fn new() -> io::Result<Self> {
        let mut mgr = Self::new_unstarted()?;
        mgr.start()?;
        Ok(mgr)
    }

    /// Create a `CameraManager` without starting it.
    ///
    /// Call [Self::start] before using it to enumerate cameras.
    pub fn new_unstarted() -> io::Result<Self> {
        let ptr = NonNull::new(unsafe { libcamera_camera_manager_create() }).unwrap();
        Ok(CameraManager {
            ptr,
            callbacks: Box::new(ManagerCallbacks {
                added: None,
                removed: None,
                hotplug_tx: None,
            }),
            added_handle: core::ptr::null_mut(),
            removed_handle: core::ptr::null_mut(),
            started: false,
            tracker: Arc::new(CameraTracker::default()),
        })
    }

    /// Start the camera manager if it is not already running.
    pub fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        let ret = unsafe { libcamera_camera_manager_start(self.ptr.as_ptr()) };
        handle_result(ret)?;
        self.started = true;
        Ok(())
    }

    /// Stop the camera manager. Safe to call multiple times.
    pub fn stop(&mut self) -> io::Result<()> {
        if !self.started {
            return Ok(());
        }
        unsafe { libcamera_camera_manager_stop(self.ptr.as_ptr()) };
        self.started = false;
        Ok(())
    }

    /// Attempt to stop only if no tracked cameras are still alive.
    pub fn try_stop(&mut self) -> io::Result<()> {
        if self.tracker.has_live_handles() {
            return Err(io::Error::other(
                "cannot stop CameraManager while cameras are still alive",
            ));
        }
        self.stop()
    }

    /// Restart the manager by stopping (if safe) and starting again.
    pub fn restart(&mut self) -> io::Result<()> {
        self.try_stop()?;
        self.start()
    }

    /// Returns true if the manager has been started.
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// Returns version string of the linked libcamera.
    pub fn version(&self) -> &str {
        unsafe { CStr::from_ptr(libcamera_version_string()) }.to_str().unwrap()
    }

    /// Enumerates cameras within the system.
    pub fn cameras<'a>(&self) -> CameraList<'a> {
        unsafe {
            CameraList::from_ptr(
                NonNull::new(libcamera_camera_manager_cameras(self.ptr.as_ptr())).unwrap(),
                Some(self.tracker.clone()),
            )
        }
    }

    /// Returns a camera by id if present.
    pub fn get<'a>(&self, id: &str) -> Option<Camera<'a>> {
        let id_cstr = CString::new(id).ok()?;
        let cam_ptr = unsafe { libcamera_camera_manager_get_id(self.ptr.as_ptr(), id_cstr.as_ptr()) };
        NonNull::new(cam_ptr).map(|p| unsafe { Camera::from_ptr_tracked(p, Some(self.tracker.clone())) })
    }

    /// Set the log level.
    ///
    /// # Parameters
    ///
    /// * `category` - Free-form category string, a list of those can be seen by running `grep 'LOG_DEFINE_CATEGORY('
    ///   -R` on the `libcamera` source code
    /// * `level` - Maximum log importance level to show, anything more less important than that will be hidden.
    pub fn log_set_level(&self, category: &str, level: LoggingLevel) {
        let category = CString::new(category).expect("category contains null byte");
        let level: &CStr = level.into();
        unsafe {
            libcamera_log_set_level(category.as_ptr(), level.as_ptr());
        }
    }

    /// Register a callback for camera-added events.
    ///
    /// # Warning
    /// The callback is invoked on libcamera's internal thread. Do not block in the callback; send work to another
    /// thread/channel if needed.
    pub fn on_camera_added(&mut self, cb: impl FnMut(Camera<'static>) + Send + 'static) {
        self.callbacks.added = Some(Box::new(cb));
        if self.added_handle.is_null() {
            let data = self.callbacks.as_mut() as *mut _ as *mut _;
            self.added_handle = unsafe {
                libcamera_camera_manager_camera_added_connect(self.ptr.as_ptr(), Some(camera_added_cb), data)
            };
        }
    }

    /// Register a callback for camera-removed events.
    ///
    /// # Warning
    /// The callback is invoked on libcamera's internal thread. Do not block in the callback; send work to another
    /// thread/channel if needed.
    pub fn on_camera_removed(&mut self, cb: impl FnMut(Camera<'static>) + Send + 'static) {
        self.callbacks.removed = Some(Box::new(cb));
        if self.removed_handle.is_null() {
            let data = self.callbacks.as_mut() as *mut _ as *mut _;
            self.removed_handle = unsafe {
                libcamera_camera_manager_camera_removed_connect(self.ptr.as_ptr(), Some(camera_removed_cb), data)
            };
        }
    }

    /// Subscribe to hotplug events via a channel.
    ///
    /// The returned `Receiver` yields `HotplugEvent` values. Internally this hooks into the libcamera hotplug signals
    /// and forwards them; it uses the same callbacks as `on_camera_added/removed`, so do not mix different senders
    /// without care.
    pub fn subscribe_hotplug_events(&mut self) -> mpsc::Receiver<HotplugEvent> {
        let (tx, rx) = mpsc::channel();
        self.callbacks.hotplug_tx = Some(tx);
        // Ensure signals are connected
        if self.added_handle.is_null() {
            let data = self.callbacks.as_mut() as *mut _ as *mut _;
            self.added_handle = unsafe {
                libcamera_camera_manager_camera_added_connect(self.ptr.as_ptr(), Some(camera_added_cb), data)
            };
        }
        if self.removed_handle.is_null() {
            let data = self.callbacks.as_mut() as *mut _ as *mut _;
            self.removed_handle = unsafe {
                libcamera_camera_manager_camera_removed_connect(self.ptr.as_ptr(), Some(camera_removed_cb), data)
            };
        }
        rx
    }
}

impl Drop for CameraManager {
    fn drop(&mut self) {
        if self.started {
            unsafe { libcamera_camera_manager_stop(self.ptr.as_ptr()) };
            self.started = false;
        }
        unsafe {
            if !self.added_handle.is_null() {
                libcamera_camera_manager_camera_signal_disconnect(self.ptr.as_ptr(), self.added_handle);
            }
            if !self.removed_handle.is_null() {
                libcamera_camera_manager_camera_signal_disconnect(self.ptr.as_ptr(), self.removed_handle);
            }
            libcamera_camera_manager_destroy(self.ptr.as_ptr());
        }
    }
}

#[derive(Default)]
pub(crate) struct CameraTracker {
    handles: Mutex<Vec<std::sync::Weak<()>>>,
}

impl CameraTracker {
    pub(crate) fn track(&self) -> std::sync::Arc<()> {
        let token = std::sync::Arc::new(());
        self.handles.lock().unwrap().push(std::sync::Arc::downgrade(&token));
        token
    }

    fn has_live_handles(&self) -> bool {
        self.handles.lock().unwrap().iter().any(|w| w.upgrade().is_some())
    }
}

pub struct CameraList<'d> {
    ptr: NonNull<libcamera_camera_list_t>,
    _phantom: PhantomData<&'d ()>,
    tracker: Option<Arc<CameraTracker>>,
}

impl<'d> CameraList<'d> {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_camera_list_t>, tracker: Option<Arc<CameraTracker>>) -> Self {
        Self {
            ptr,
            _phantom: Default::default(),
            tracker,
        }
    }

    /// Number of cameras
    pub fn len(&self) -> usize {
        unsafe { libcamera_camera_list_size(self.ptr.as_ptr()) }
    }

    /// Returns `true` if there are no cameras available
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns camera at a given index.
    ///
    /// Returns [None] if index is out of range of available cameras.
    pub fn get(&self, index: usize) -> Option<Camera<'d>> {
        let cam_ptr = unsafe { libcamera_camera_list_get(self.ptr.as_ptr(), index as _) };
        let tracker = self.tracker.clone();
        NonNull::new(cam_ptr).map(|p| unsafe { Camera::from_ptr_tracked(p, tracker) })
    }

    /// Returns an iterator over the cameras in the list.
    pub fn iter(&'d self) -> CameraListIter<'d> {
        CameraListIter { list: self, index: 0 }
    }
}

impl Drop for CameraList<'_> {
    fn drop(&mut self) {
        unsafe {
            libcamera_camera_list_destroy(self.ptr.as_ptr());
        }
    }
}

pub struct CameraListIter<'d> {
    list: &'d CameraList<'d>,
    index: usize,
}

impl<'d> Iterator for CameraListIter<'d> {
    type Item = Camera<'d>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.list.len() {
            let camera = self.list.get(self.index);
            self.index += 1;
            camera
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.list.len().saturating_sub(self.index);
        (len, Some(len))
    }
}

impl<'d> ExactSizeIterator for CameraListIter<'d> {}

unsafe extern "C" fn camera_added_cb(data: *mut core::ffi::c_void, cam: *mut libcamera_camera_t) {
    if data.is_null() || cam.is_null() {
        return;
    }
    // Safety: called from libcamera thread, user must ensure callbacks are Send-safe.
    let state = &mut *(data as *mut ManagerCallbacks);
    if let Some(ptr) = NonNull::new(cam) {
        // Clone shared_ptr once to avoid double-drop when used by multiple consumers.
        let cam_copy = unsafe { libcamera_camera_copy(ptr.as_ptr()) };
        if let Some(copy_ptr) = NonNull::new(cam_copy) {
            let cam = unsafe { Camera::from_ptr(copy_ptr) };
            let cam_id = cam.id().to_string();
            if let Some(cb) = state.added.as_mut() {
                cb(cam);
            } else {
                drop(cam);
            }
            if let Some(tx) = state.hotplug_tx.as_ref() {
                let _ = tx.send(HotplugEvent::Added(cam_id));
            }
        }
    }
}

unsafe extern "C" fn camera_removed_cb(data: *mut core::ffi::c_void, cam: *mut libcamera_camera_t) {
    if data.is_null() || cam.is_null() {
        return;
    }
    let state = &mut *(data as *mut ManagerCallbacks);
    if let Some(ptr) = NonNull::new(cam) {
        let cam_copy = unsafe { libcamera_camera_copy(ptr.as_ptr()) };
        if let Some(copy_ptr) = NonNull::new(cam_copy) {
            let cam = unsafe { Camera::from_ptr(copy_ptr) };
            let cam_id = cam.id().to_string();
            if let Some(cb) = state.removed.as_mut() {
                cb(cam);
            } else {
                drop(cam);
            }
            if let Some(tx) = state.hotplug_tx.as_ref() {
                let _ = tx.send(HotplugEvent::Removed(cam_id));
            }
        }
    }
}
