//! Media diff mode — images, audio and video shown as old → new players
//! instead of a text diff. Lives alongside the single-file text modes: the
//! mode state (`DirtyFileDiffState` etc.) still tracks *which* file is open,
//! while `MediaDiffState` owns the decoded payloads, viewer settings and
//! playback handles.

use std::sync::Arc;
use std::time::Instant;

use iced::{Size, Task};

use crate::{
    message::Message,
    services::{
        media::{
            audio::AudioClip,
            engine::{Voice, VoiceSource},
            image::{DecodedImage, DifferenceImage},
            video::VideoPlayer,
            DecodedMedia, MediaDiffSources, MediaError, MediaKind, MediaSide, MediaSource,
        },
        GitError, MediaDiffRequest,
    },
    widgets::media::{
        image_viewer::{self, AnimationPlayback, ImageView, ImageViewerEvent},
        timeline::{TimelineEvent, TransportSource},
        video_surface::VideoSurfaceEvent,
    },
    work::{git_read_work, presentation_work},
};

use super::super::super::{panel_messages::DiffPanelAction, RepositoryMessage};
use super::super::ScreenCtx;
use super::DiffPanel;

/// Seek step for ←/→ on audio/video.
pub const SEEK_STEP_SECS: f64 = 5.0;
const ZOOM_STEP: f32 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareMode {
    SideBySide,
    Swipe,
    OnionSkin,
    Difference,
}

impl CompareMode {
    pub fn label(self) -> &'static str {
        match self {
            CompareMode::SideBySide => "Side by side",
            CompareMode::Swipe => "Swipe",
            CompareMode::OnionSkin => "Onion skin",
            CompareMode::Difference => "Difference",
        }
    }

    pub fn needs_both_images(self) -> bool {
        !matches!(self, CompareMode::SideBySide)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentReason {
    /// The file was added in this change.
    NoPreviousVersion,
    /// The file was deleted in this change.
    Deleted,
}

impl AbsentReason {
    pub fn title(self) -> &'static str {
        match self {
            AbsentReason::NoPreviousVersion => "No previous version",
            AbsentReason::Deleted => "File deleted",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            AbsentReason::NoPreviousVersion => {
                "This file is new in this change, so there is nothing to show on the old side."
            }
            AbsentReason::Deleted => "This change removes the file; only the old version can be shown.",
        }
    }
}

/// An audio clip bound to an output voice.
pub struct AudioPlayer {
    pub clip: Arc<AudioClip>,
    pub voice: Voice,
}

impl AudioPlayer {
    fn new(clip: Arc<AudioClip>) -> Self {
        let voice = Voice::new(VoiceSource::Clip(clip.clone()));
        Self { clip, voice }
    }
}

impl std::fmt::Debug for AudioPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioPlayer")
            .field("duration", &self.clip.duration_secs())
            .finish()
    }
}

impl TransportSource for AudioPlayer {
    fn position_secs(&self) -> f64 {
        self.voice.position_secs().min(self.clip.duration_secs())
    }

    fn duration_secs(&self) -> f64 {
        self.clip.duration_secs()
    }

    fn is_playing(&self) -> bool {
        self.voice.is_playing()
    }
}

impl TransportSource for VideoPlayer {
    fn position_secs(&self) -> f64 {
        VideoPlayer::position_secs(self)
    }

    fn duration_secs(&self) -> f64 {
        VideoPlayer::duration_secs(self)
    }

    fn is_playing(&self) -> bool {
        VideoPlayer::is_playing(self)
    }

    fn is_buffering(&self) -> bool {
        VideoPlayer::is_buffering(self)
    }

    fn buffered_until_secs(&self) -> Option<f64> {
        let buffered = self.audio_buffered_secs()?;
        // Buffer is measured from the pipeline start; approximate with the
        // playhead as origin when we can't know the exact start.
        Some(VideoPlayer::position_secs(self) + buffered.max(0.0))
    }

    fn frame_interval_secs(&self) -> Option<f64> {
        Some(self.info().frame_interval_secs())
    }
}

/// Transport view of an animated image (frame timeline).
pub struct AnimationTransport {
    pub image: Arc<DecodedImage>,
    pub playback: AnimationPlayback,
}

impl TransportSource for AnimationTransport {
    fn position_secs(&self) -> f64 {
        self.playback.phase_ms(&self.image, Instant::now()) as f64 / 1000.0
    }

    fn duration_secs(&self) -> f64 {
        self.image.total_duration_ms as f64 / 1000.0
    }

    fn is_playing(&self) -> bool {
        self.playback.playing && self.playback.anchor.is_some()
    }

    fn frame_interval_secs(&self) -> Option<f64> {
        None
    }
}

pub enum MediaSideState {
    Loading,
    Absent(AbsentReason),
    Failed {
        message: String,
        size: Option<u64>,
    },
    Image {
        image: Arc<DecodedImage>,
        playback: AnimationPlayback,
    },
    Audio(Arc<AudioPlayer>),
    Video(Arc<VideoPlayer>),
}

impl MediaSideState {
    pub fn image(&self) -> Option<&Arc<DecodedImage>> {
        match self {
            MediaSideState::Image { image, .. } => Some(image),
            _ => None,
        }
    }

    pub fn kind(&self) -> Option<MediaKind> {
        match self {
            MediaSideState::Image { .. } => Some(MediaKind::Image),
            MediaSideState::Audio(_) => Some(MediaKind::Audio),
            MediaSideState::Video(_) => Some(MediaKind::Video),
            _ => None,
        }
    }

    pub fn properties(&self) -> Vec<(String, String)> {
        match self {
            MediaSideState::Image { image, .. } => image.properties(),
            MediaSideState::Audio(player) => player.clip.properties(),
            MediaSideState::Video(player) => player.info().properties(),
            _ => Vec::new(),
        }
    }

    fn is_playing(&self) -> bool {
        match self {
            MediaSideState::Image { playback, image } => {
                image.is_animated() && playback.playing && playback.anchor.is_some()
            }
            MediaSideState::Audio(player) => player.voice.is_playing(),
            MediaSideState::Video(player) => player.is_playing(),
            _ => false,
        }
    }
}

impl std::fmt::Debug for MediaSideState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaSideState::Loading => write!(f, "Loading"),
            MediaSideState::Absent(r) => write!(f, "Absent({r:?})"),
            MediaSideState::Failed { message, .. } => write!(f, "Failed({message})"),
            MediaSideState::Image { image, .. } => write!(f, "Image({}x{})", image.width, image.height),
            MediaSideState::Audio(p) => write!(f, "Audio({:.2}s)", p.clip.duration_secs()),
            MediaSideState::Video(p) => write!(f, "Video({:.2}s)", p.duration_secs()),
        }
    }
}

#[derive(Debug)]
pub enum DifferenceState {
    NotComputed,
    Computing,
    Ready(Arc<DifferenceImage>),
    Failed(String),
}

#[derive(Debug)]
pub struct ImageCompareState {
    pub mode: CompareMode,
    /// View for the new pane (and both panes when linked).
    pub view: ImageView,
    /// View for the old pane when views are unlinked.
    pub old_view: ImageView,
    pub linked_views: bool,
    pub effective_scale: Option<f32>,
    pub pane_size: Option<Size>,
    pub checkerboard: bool,
    pub nearest: bool,
    pub pixel_grid: bool,
    pub inspector: bool,
    pub swipe: f32,
    pub onion: f32,
    pub difference: DifferenceState,
}

impl Default for ImageCompareState {
    fn default() -> Self {
        Self {
            mode: CompareMode::SideBySide,
            view: ImageView::fit(),
            old_view: ImageView::fit(),
            linked_views: true,
            effective_scale: None,
            pane_size: None,
            checkerboard: true,
            nearest: false,
            pixel_grid: false,
            inspector: false,
            swipe: 0.5,
            onion: 0.5,
            difference: DifferenceState::NotComputed,
        }
    }
}

pub struct MediaDiffState {
    pub kind: MediaKind,
    pub generation: u64,
    pub file_path: String,
    pub old: MediaSideState,
    pub new: MediaSideState,
    pub image: ImageCompareState,
    pub focused_side: MediaSide,
    pub linked_playback: bool,
    pub show_info: bool,
    /// Volume shared by both sides (0..1).
    pub volume: f32,
    pub muted: bool,
    pub looping: bool,
    pub rate: f32,
}

impl std::fmt::Debug for MediaDiffState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaDiffState")
            .field("kind", &self.kind)
            .field("generation", &self.generation)
            .field("file_path", &self.file_path)
            .field("old", &self.old)
            .field("new", &self.new)
            .finish()
    }
}

impl MediaDiffState {
    pub fn side(&self, side: MediaSide) -> &MediaSideState {
        match side {
            MediaSide::Old => &self.old,
            MediaSide::New => &self.new,
        }
    }

    pub fn side_mut(&mut self, side: MediaSide) -> &mut MediaSideState {
        match side {
            MediaSide::Old => &mut self.old,
            MediaSide::New => &mut self.new,
        }
    }

    pub fn both_images(&self) -> Option<(&Arc<DecodedImage>, &Arc<DecodedImage>)> {
        Some((self.old.image()?, self.new.image()?))
    }

    /// Effective kind once decoded (sniffing may override the extension).
    pub fn effective_kind(&self) -> MediaKind {
        self.new
            .kind()
            .or_else(|| self.old.kind())
            .unwrap_or(self.kind)
    }

    pub fn view_for(&self, side: MediaSide) -> ImageView {
        if self.image.linked_views || side == MediaSide::New {
            self.image.view
        } else {
            self.image.old_view
        }
    }

    pub fn any_playing(&self) -> bool {
        self.old.is_playing() || self.new.is_playing()
    }

    /// Side whose transport keyboard shortcuts act on.
    fn effective_focus(&self) -> MediaSide {
        match (self.focused_side, &self.new, &self.old) {
            (MediaSide::New, MediaSideState::Absent(_) | MediaSideState::Failed { .. }, _) => {
                MediaSide::Old
            }
            (MediaSide::Old, _, MediaSideState::Absent(_) | MediaSideState::Failed { .. }) => {
                MediaSide::New
            }
            (side, _, _) => side,
        }
    }

    fn primary_image_dims(&self) -> Option<(f32, f32)> {
        let img = self.new.image().or_else(|| self.old.image())?;
        Some((img.width as f32, img.height as f32))
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportCommand {
    TogglePlay,
    Play,
    Pause,
    /// Seek to 0 and pause.
    Stop,
    Seek(f64),
    SeekRelative(f64),
    SeekEnd,
    StepFrame(i32),
    SetVolume(f32),
    ToggleMute,
    ToggleLoop,
    SetRate(f32),
}

#[derive(Debug, Clone)]
pub enum MediaAction {
    Load {
        generation: u64,
        request: MediaDiffRequest,
    },
    SourcesLoaded {
        generation: u64,
        result: Result<MediaDiffSources, GitError>,
    },
    SideDecoded {
        generation: u64,
        side: MediaSide,
        result: Result<DecodedMedia, MediaError>,
    },
    DifferenceReady {
        generation: u64,
        result: Result<Arc<DifferenceImage>, String>,
    },
    SetCompareMode(CompareMode),
    Viewer {
        side: MediaSide,
        event: ImageViewerEvent,
    },
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomActual,
    ToggleLinkedViews,
    ToggleCheckerboard,
    ToggleNearest,
    TogglePixelGrid,
    ToggleInspector,
    ToggleInfo,
    SetSwipe(f32),
    SetOnion(f32),
    Transport {
        side: MediaSide,
        command: TransportCommand,
    },
    /// Keyboard transport: acts on the focused side (or both when linked).
    KeyTransport(TransportCommand),
    Timeline {
        side: MediaSide,
        event: TimelineEvent,
    },
    Surface {
        side: MediaSide,
        event: VideoSurfaceEvent,
    },
    ToggleLinkedPlayback,
    FocusSide(MediaSide),
    ViewAsText,
    /// The text loader found media bytes behind a non-media extension.
    SwitchToMedia {
        kind: MediaKind,
    },
    /// Playback state changed inside a widget; re-render only.
    Refresh,
}

// ---------------------------------------------------------------------------
// DiffPanel integration
// ---------------------------------------------------------------------------

impl DiffPanel {
    /// Decide whether `path` should open in the media viewer. Honors the
    /// per-file "view as text" override.
    pub(super) fn media_kind_for_open(&self, path: &str) -> Option<MediaKind> {
        if self.force_text_path.as_deref() == Some(path) {
            return None;
        }
        crate::services::media::media_kind_from_path(path)
    }

    /// Replace any media state with a fresh loading state for `path` and
    /// return the load action.
    pub(super) fn begin_media_load(
        &mut self,
        kind: MediaKind,
        request: MediaDiffRequest,
    ) -> DiffPanelAction {
        let generation = self.next_diff_generation();
        // Keep viewer settings when reloading the same file (e.g. the
        // working tree changed underneath us) so zoom/mode don't reset.
        let previous = self.media.take();
        let path = request.path().to_string();
        let (image, focused_side, linked_playback, show_info, volume, muted, looping, rate) =
            match previous {
                Some(prev) if prev.file_path == path => (
                    ImageCompareState {
                        difference: DifferenceState::NotComputed,
                        effective_scale: None,
                        ..prev.image
                    },
                    prev.focused_side,
                    prev.linked_playback,
                    prev.show_info,
                    prev.volume,
                    prev.muted,
                    prev.looping,
                    prev.rate,
                ),
                _ => (
                    ImageCompareState::default(),
                    MediaSide::New,
                    false,
                    false,
                    1.0,
                    false,
                    false,
                    1.0,
                ),
            };
        self.media = Some(MediaDiffState {
            kind,
            generation,
            file_path: path,
            old: MediaSideState::Loading,
            new: MediaSideState::Loading,
            image,
            focused_side,
            linked_playback,
            show_info,
            volume,
            muted,
            looping,
            rate,
        });
        DiffPanelAction::Media(MediaAction::Load {
            generation,
            request,
        })
    }

    /// The media request matching the active text mode state, if any.
    pub(super) fn media_request_for_active(&self, kind: MediaKind) -> Option<MediaDiffRequest> {
        if let Some(state) = &self.dirty_file_diff {
            return Some(MediaDiffRequest::Dirty {
                path: state.file_path.clone(),
                is_staged: state.is_staged,
                kind,
            });
        }
        if let Some(state) = &self.commit_file_diff {
            return Some(MediaDiffRequest::Commit {
                commit_hash: state.commit_hash.clone(),
                path: state.file_path.clone(),
                kind,
            });
        }
        if let Some(state) = &self.merged_file_diff {
            return Some(MediaDiffRequest::Merged {
                hashes: state.hashes.clone(),
                path: state.file_path.clone(),
                kind,
            });
        }
        None
    }

    /// Switch the active file to the media viewer (used when the text loader
    /// sniffed media content behind a non-media extension).
    pub(super) fn switch_active_to_media(&mut self, kind: MediaKind) -> Option<DiffPanelAction> {
        let request = self.media_request_for_active(kind)?;
        Some(self.begin_media_load(kind, request))
    }

    /// Text load action for the active mode (used by "View as text").
    fn text_load_action_for_active(&mut self) -> Option<DiffPanelAction> {
        if let Some(state) = self.dirty_file_diff.as_mut() {
            let generation = self.next_generation;
            self.next_generation = self.next_generation.wrapping_add(1).max(1);
            state.render_generation = generation;
            state.render_data = None;
            return Some(DiffPanelAction::LoadDirtyFileDiff {
                generation,
                path: state.file_path.clone(),
                is_staged: state.is_staged,
            });
        }
        if let Some(state) = self.commit_file_diff.as_mut() {
            let generation = self.next_generation;
            self.next_generation = self.next_generation.wrapping_add(1).max(1);
            state.render_generation = generation;
            state.render_data = None;
            return Some(DiffPanelAction::LoadCommitFileDiff {
                generation,
                commit_hash: state.commit_hash.clone(),
                path: state.file_path.clone(),
            });
        }
        if let Some(state) = self.merged_file_diff.as_mut() {
            let generation = self.next_generation;
            self.next_generation = self.next_generation.wrapping_add(1).max(1);
            state.render_generation = generation;
            state.render_data = None;
            return Some(DiffPanelAction::LoadMergedFileDiff {
                generation,
                hashes: state.hashes.clone(),
                path: state.file_path.clone(),
            });
        }
        None
    }

    pub(super) fn update_media(
        &mut self,
        action: MediaAction,
        ctx: &mut ScreenCtx<'_>,
    ) -> Task<Message> {
        match action {
            MediaAction::Load {
                generation,
                request,
            } => {
                let repo = ctx.repository.clone();
                let tab_id = ctx.tab_id;
                let path_for_log = request.path().to_string();
                Task::perform(
                    git_read_work(move || {
                        let span = crate::perf::Span::new("git.media_diff_load")
                            .field("tab", tab_id)
                            .field("path", &path_for_log)
                            .field("kind", request.kind().label());
                        let result = repo.load_media_diff_sources(&request);
                        match &result {
                            Ok(sources) => span
                                .field("old_bytes", sources.old.byte_len().unwrap_or(0))
                                .finish_with("new_bytes", sources.new.byte_len().unwrap_or(0)),
                            Err(_) => span.finish_with("outcome", "err"),
                        }
                        result
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::DiffPanel(DiffPanelAction::Media(
                                MediaAction::SourcesLoaded { generation, result },
                            )),
                        )
                    },
                )
            }
            MediaAction::SourcesLoaded { generation, result } => {
                self.on_media_sources_loaded(generation, result, ctx.tab_id)
            }
            MediaAction::SideDecoded {
                generation,
                side,
                result,
            } => {
                self.on_media_side_decoded(generation, side, result);
                self.maybe_compute_difference(ctx.tab_id)
            }
            MediaAction::DifferenceReady { generation, result } => {
                if let Some(state) = self.media.as_mut() {
                    if state.generation == generation {
                        state.image.difference = match result {
                            Ok(diff) => DifferenceState::Ready(diff),
                            Err(msg) => DifferenceState::Failed(msg),
                        };
                    }
                }
                Task::none()
            }
            MediaAction::SetCompareMode(mode) => {
                if let Some(state) = self.media.as_mut() {
                    if mode.needs_both_images() && state.both_images().is_none() {
                        return Task::none();
                    }
                    state.image.mode = mode;
                    // Overlay modes need a shared transform.
                    if mode.needs_both_images() {
                        state.image.linked_views = true;
                    }
                }
                self.maybe_compute_difference(ctx.tab_id)
            }
            MediaAction::Viewer { side, event } => {
                self.on_viewer_event(side, event);
                Task::none()
            }
            MediaAction::ZoomIn => {
                self.zoom_step(ZOOM_STEP);
                Task::none()
            }
            MediaAction::ZoomOut => {
                self.zoom_step(1.0 / ZOOM_STEP);
                Task::none()
            }
            MediaAction::ZoomFit => {
                if let Some(state) = self.media.as_mut() {
                    state.image.view = ImageView::fit();
                    state.image.old_view = ImageView::fit();
                }
                Task::none()
            }
            MediaAction::ZoomActual => {
                if let Some(state) = self.media.as_mut() {
                    let actual = ImageView {
                        scale: Some(1.0),
                        center: (0.5, 0.5),
                    };
                    state.image.view = actual;
                    state.image.old_view = actual;
                }
                Task::none()
            }
            MediaAction::ToggleLinkedViews => {
                if let Some(state) = self.media.as_mut() {
                    state.image.linked_views = !state.image.linked_views;
                    if state.image.linked_views {
                        state.image.old_view = state.image.view;
                    }
                }
                Task::none()
            }
            MediaAction::ToggleCheckerboard => {
                if let Some(state) = self.media.as_mut() {
                    state.image.checkerboard = !state.image.checkerboard;
                }
                Task::none()
            }
            MediaAction::ToggleNearest => {
                if let Some(state) = self.media.as_mut() {
                    state.image.nearest = !state.image.nearest;
                }
                Task::none()
            }
            MediaAction::TogglePixelGrid => {
                if let Some(state) = self.media.as_mut() {
                    state.image.pixel_grid = !state.image.pixel_grid;
                }
                Task::none()
            }
            MediaAction::ToggleInspector => {
                if let Some(state) = self.media.as_mut() {
                    state.image.inspector = !state.image.inspector;
                }
                Task::none()
            }
            MediaAction::ToggleInfo => {
                if let Some(state) = self.media.as_mut() {
                    state.show_info = !state.show_info;
                }
                Task::none()
            }
            MediaAction::SetSwipe(t) => {
                if let Some(state) = self.media.as_mut() {
                    state.image.swipe = t.clamp(0.0, 1.0);
                }
                Task::none()
            }
            MediaAction::SetOnion(t) => {
                if let Some(state) = self.media.as_mut() {
                    state.image.onion = t.clamp(0.0, 1.0);
                }
                Task::none()
            }
            MediaAction::Transport { side, command } => {
                self.apply_transport(Some(side), command);
                Task::none()
            }
            MediaAction::KeyTransport(command) => {
                self.apply_transport(None, command);
                Task::none()
            }
            MediaAction::Timeline { side, event } => {
                match event {
                    TimelineEvent::Seek(secs) => {
                        self.apply_transport(Some(side), TransportCommand::Seek(secs))
                    }
                    TimelineEvent::PlaybackStateChanged => {}
                }
                Task::none()
            }
            MediaAction::Surface { side, event } => {
                match event {
                    VideoSurfaceEvent::TogglePlay => {
                        self.apply_transport(Some(side), TransportCommand::TogglePlay)
                    }
                    VideoSurfaceEvent::PlaybackStateChanged => {}
                }
                Task::none()
            }
            MediaAction::ToggleLinkedPlayback => {
                if let Some(state) = self.media.as_mut() {
                    state.linked_playback = !state.linked_playback;
                }
                Task::none()
            }
            MediaAction::FocusSide(side) => {
                if let Some(state) = self.media.as_mut() {
                    state.focused_side = side;
                }
                Task::none()
            }
            MediaAction::ViewAsText => {
                let path = self.media.as_ref().map(|m| m.file_path.clone());
                self.media = None;
                self.force_text_path = path;
                match self.text_load_action_for_active() {
                    Some(action) => super::update::update(self, action, ctx),
                    None => Task::none(),
                }
            }
            MediaAction::SwitchToMedia { kind } => {
                if self.media.is_some() {
                    return Task::none();
                }
                match self.switch_active_to_media(kind) {
                    Some(action) => super::update::update(self, action, ctx),
                    None => Task::none(),
                }
            }
            MediaAction::Refresh => Task::none(),
        }
    }

    fn on_media_sources_loaded(
        &mut self,
        generation: u64,
        result: Result<MediaDiffSources, GitError>,
        tab_id: crate::core::TabId,
    ) -> Task<Message> {
        let Some(state) = self.media.as_mut() else {
            return Task::none();
        };
        if state.generation != generation {
            return Task::none();
        }
        let sources = match result {
            Ok(sources) => sources,
            Err(err) => {
                let message = format!("Could not load file contents: {err}");
                state.old = MediaSideState::Failed {
                    message: message.clone(),
                    size: None,
                };
                state.new = MediaSideState::Failed {
                    message,
                    size: None,
                };
                return Task::none();
            }
        };
        if sources.file_path != state.file_path {
            return Task::none();
        }
        let kind = state.kind;
        let path = state.file_path.clone();
        let mut tasks = Vec::new();
        for (side, source) in [(MediaSide::Old, sources.old), (MediaSide::New, sources.new)] {
            match &source {
                MediaSource::Missing => {
                    let reason = match side {
                        MediaSide::Old => AbsentReason::NoPreviousVersion,
                        MediaSide::New => AbsentReason::Deleted,
                    };
                    *state.side_mut(side) = MediaSideState::Absent(reason);
                }
                MediaSource::TooLarge { bytes, max } => {
                    *state.side_mut(side) = MediaSideState::Failed {
                        message: MediaError::TooLarge {
                            bytes: *bytes,
                            max: *max,
                        }
                        .to_string(),
                        size: Some(*bytes),
                    };
                }
                _ => {
                    let path = path.clone();
                    let size = source.byte_len();
                    tasks.push(Task::perform(
                        presentation_work(move || {
                            if matches!(kind, MediaKind::Audio | MediaKind::Video) {
                                // Open the output device off the UI thread so
                                // the first "play" is instant.
                                crate::services::media::engine::engine().ensure_started();
                            }
                            crate::services::media::decode_side(kind, &source, &path)
                        }),
                        move |result| {
                            let result = match result {
                                Some(result) => result,
                                None => Err(MediaError::Corrupt(
                                    "decoder crashed while reading the file".to_string(),
                                )),
                            };
                            let _ = size;
                            Message::tab(
                                tab_id,
                                RepositoryMessage::DiffPanel(DiffPanelAction::Media(
                                    MediaAction::SideDecoded {
                                        generation,
                                        side,
                                        result,
                                    },
                                )),
                            )
                        },
                    ));
                }
            }
        }
        // A side that never had content resolves immediately; the other
        // decodes in the background.
        Task::batch(tasks)
    }

    fn on_media_side_decoded(
        &mut self,
        generation: u64,
        side: MediaSide,
        result: Result<DecodedMedia, MediaError>,
    ) {
        let Some(state) = self.media.as_mut() else {
            return;
        };
        if state.generation != generation {
            return;
        }
        let side_state = match result {
            Ok(DecodedMedia::Image(image)) => {
                let playback = AnimationPlayback {
                    playing: image.is_animated(),
                    anchor: image.is_animated().then(|| (Instant::now(), 0)),
                    paused_frame: 0,
                };
                MediaSideState::Image { image, playback }
            }
            Ok(DecodedMedia::Audio(clip)) => {
                let player = AudioPlayer::new(clip);
                player.voice.set_volume(state.volume);
                player.voice.set_muted(state.muted);
                player.voice.set_looping(state.looping);
                MediaSideState::Audio(Arc::new(player))
            }
            Ok(DecodedMedia::Video(player)) => {
                player.set_volume(state.volume);
                player.set_muted(state.muted);
                player.set_looping(state.looping);
                player.set_rate(state.rate);
                MediaSideState::Video(player)
            }
            Err(err) => MediaSideState::Failed {
                message: err.to_string(),
                size: None,
            },
        };
        *state.side_mut(side) = side_state;

        // Animated images on both sides start in lock-step.
        if let (
            MediaSideState::Image {
                playback: old_pb,
                image: old_img,
            },
            MediaSideState::Image {
                playback: new_pb,
                image: new_img,
            },
        ) = (&mut state.old, &mut state.new)
        {
            if old_img.is_animated() && new_img.is_animated() {
                let now = Instant::now();
                old_pb.anchor = Some((now, 0));
                new_pb.anchor = Some((now, 0));
            }
        }
        if state.image.mode.needs_both_images() && state.both_images().is_none() {
            state.image.mode = CompareMode::SideBySide;
        }
    }

    fn maybe_compute_difference(&mut self, tab_id: crate::core::TabId) -> Task<Message> {
        let Some(state) = self.media.as_mut() else {
            return Task::none();
        };
        if state.image.mode != CompareMode::Difference
            || !matches!(state.image.difference, DifferenceState::NotComputed)
        {
            return Task::none();
        }
        let Some((old, new)) = state.both_images() else {
            return Task::none();
        };
        let (old, new) = (old.clone(), new.clone());
        let generation = state.generation;
        state.image.difference = DifferenceState::Computing;
        Task::perform(
            presentation_work(move || {
                let span = crate::perf::Span::new("cpu.image_difference")
                    .field("width", new.width)
                    .field("height", new.height);
                let result = crate::services::media::image::difference_image(&old, &new);
                span.finish_with("ok", result.is_ok());
                result.map(Arc::new)
            }),
            move |result| {
                let result = match result {
                    Some(result) => result,
                    None => Err("Difference computation failed.".to_string()),
                };
                Message::tab(
                    tab_id,
                    RepositoryMessage::DiffPanel(DiffPanelAction::Media(
                        MediaAction::DifferenceReady { generation, result },
                    )),
                )
            },
        )
    }

    fn on_viewer_event(&mut self, side: MediaSide, event: ImageViewerEvent) {
        let Some(state) = self.media.as_mut() else {
            return;
        };
        match event {
            ImageViewerEvent::ViewChanged(view) => {
                if state.image.linked_views || side == MediaSide::New {
                    state.image.view = view;
                    if state.image.linked_views {
                        state.image.old_view = view;
                    }
                } else {
                    state.image.old_view = view;
                }
                state.focused_side = side;
            }
            ImageViewerEvent::ToggleFit => {
                let current = state.view_for(side);
                let next = if current.is_fit() {
                    ImageView {
                        scale: Some(1.0),
                        center: current.center,
                    }
                } else {
                    ImageView::fit()
                };
                if state.image.linked_views || side == MediaSide::New {
                    state.image.view = next;
                    if state.image.linked_views {
                        state.image.old_view = next;
                    }
                } else {
                    state.image.old_view = next;
                }
            }
            ImageViewerEvent::SwipeMoved(t) => state.image.swipe = t.clamp(0.0, 1.0),
            ImageViewerEvent::ScaleReported { scale, pane } => {
                if side == MediaSide::New || state.new.image().is_none() {
                    state.image.effective_scale = Some(scale);
                    state.image.pane_size = Some(pane);
                }
            }
            ImageViewerEvent::FrameChanged { .. } => {
                // The canvas keeps its own clock; the message only exists to
                // refresh the frame counter in the caption.
            }
        }
    }

    fn zoom_step(&mut self, factor: f32) {
        let Some(state) = self.media.as_mut() else {
            return;
        };
        let Some((iw, ih)) = state.primary_image_dims() else {
            return;
        };
        let pane = state.image.pane_size.unwrap_or(Size::new(800.0, 600.0));
        let side = state.effective_focus();
        let current = state.view_for(side);
        let next = image_viewer::zoom_centered(current, factor, pane, iw, ih);
        if state.image.linked_views {
            state.image.view = next;
            state.image.old_view = next;
        } else if side == MediaSide::New {
            state.image.view = next;
        } else {
            state.image.old_view = next;
        }
    }

    /// Apply a transport command to one side, or — when `side` is `None`
    /// (keyboard) — to the focused side, and to both when playback is linked.
    fn apply_transport(&mut self, side: Option<MediaSide>, command: TransportCommand) {
        let Some(state) = self.media.as_mut() else {
            return;
        };
        let target = side.unwrap_or_else(|| state.effective_focus());
        if let Some(side) = side {
            state.focused_side = side;
        }
        let both = state.linked_playback
            || matches!(
                command,
                TransportCommand::SetVolume(_)
                    | TransportCommand::ToggleMute
                    | TransportCommand::ToggleLoop
                    | TransportCommand::SetRate(_)
            );
        // Shared settings are stored once and mirrored to both players.
        match command {
            TransportCommand::SetVolume(v) => state.volume = v.clamp(0.0, 1.0),
            TransportCommand::ToggleMute => state.muted = !state.muted,
            TransportCommand::ToggleLoop => state.looping = !state.looping,
            TransportCommand::SetRate(r) => state.rate = r.clamp(0.25, 4.0),
            _ => {}
        }
        let now = Instant::now();
        let sides: Vec<MediaSide> = if both {
            vec![MediaSide::Old, MediaSide::New]
        } else {
            vec![target]
        };
        // Exclusive playback: starting one side pauses the other unless
        // playback is linked (A/B listening would otherwise mix both).
        let starts_playback = matches!(
            command,
            TransportCommand::Play | TransportCommand::TogglePlay
        ) && !state.side(target).is_playing();
        if starts_playback && !state.linked_playback {
            let other = target.other();
            apply_to_side(state, other, TransportCommand::Pause, now);
        }
        for side in sides {
            apply_to_side(state, side, command, now);
        }
    }
}

fn apply_to_side(state: &mut MediaDiffState, side: MediaSide, command: TransportCommand, now: Instant) {
    let volume = state.volume;
    let muted = state.muted;
    let looping = state.looping;
    let rate = state.rate;
    match state.side_mut(side) {
        MediaSideState::Image { image, playback } => {
            if !image.is_animated() {
                return;
            }
            let last = image.frame_count().saturating_sub(1);
            match command {
                TransportCommand::TogglePlay => {
                    if playback.playing && playback.anchor.is_some() {
                        playback.pause(image, now);
                    } else {
                        playback.play(image, now);
                    }
                }
                TransportCommand::Play => playback.play(image, now),
                TransportCommand::Pause => playback.pause(image, now),
                TransportCommand::Stop => {
                    playback.pause(image, now);
                    playback.seek_frame(image, 0, now);
                }
                TransportCommand::Seek(secs) => {
                    let ms = (secs * 1000.0).max(0.0) as u32;
                    let frame = image.frame_index_at(ms.min(image.total_duration_ms.saturating_sub(1)));
                    playback.seek_frame(image, frame, now);
                }
                TransportCommand::SeekRelative(delta) => {
                    let step = if delta < 0.0 { -1i64 } else { 1i64 };
                    let current = playback.frame_at(image, now) as i64;
                    let next = (current + step).rem_euclid(image.frame_count() as i64) as usize;
                    playback.pause(image, now);
                    playback.seek_frame(image, next, now);
                }
                TransportCommand::SeekEnd => {
                    playback.pause(image, now);
                    playback.seek_frame(image, last, now);
                }
                TransportCommand::StepFrame(delta) => {
                    let current = playback.frame_at(image, now) as i64;
                    let next = (current + delta as i64).rem_euclid(image.frame_count() as i64) as usize;
                    playback.pause(image, now);
                    playback.seek_frame(image, next, now);
                }
                TransportCommand::SetVolume(_)
                | TransportCommand::ToggleMute
                | TransportCommand::ToggleLoop
                | TransportCommand::SetRate(_) => {}
            }
        }
        MediaSideState::Audio(player) => {
            let voice = &player.voice;
            match command {
                TransportCommand::TogglePlay => {
                    if voice.has_ended() && !voice.is_playing() {
                        voice.seek_secs(0.0);
                    }
                    voice.toggle();
                }
                TransportCommand::Play => {
                    if voice.has_ended() {
                        voice.seek_secs(0.0);
                    }
                    voice.play();
                }
                TransportCommand::Pause => voice.pause(),
                TransportCommand::Stop => {
                    voice.pause();
                    voice.seek_secs(0.0);
                }
                TransportCommand::Seek(secs) => voice.seek_secs(secs),
                TransportCommand::SeekRelative(delta) => {
                    voice.seek_secs((voice.position_secs() + delta).max(0.0));
                }
                TransportCommand::SeekEnd => {
                    voice.pause();
                    voice.seek_secs(player.clip.duration_secs());
                }
                TransportCommand::StepFrame(delta) => {
                    // Audio has no frames; nudge by 10 ms.
                    voice.seek_secs((voice.position_secs() + delta as f64 * 0.01).max(0.0));
                }
                TransportCommand::SetVolume(_) => voice.set_volume(volume),
                TransportCommand::ToggleMute => voice.set_muted(muted),
                TransportCommand::ToggleLoop => voice.set_looping(looping),
                TransportCommand::SetRate(_) => {
                    // Audio speed is intentionally not exposed (pitch shift).
                    let _ = rate;
                }
            }
        }
        MediaSideState::Video(player) => match command {
            TransportCommand::TogglePlay => player.toggle(),
            TransportCommand::Play => player.play(),
            TransportCommand::Pause => player.pause(),
            TransportCommand::Stop => {
                player.pause();
                player.seek(0.0);
            }
            TransportCommand::Seek(secs) => player.seek(secs),
            TransportCommand::SeekRelative(delta) => player.seek_relative(delta),
            TransportCommand::SeekEnd => {
                player.pause();
                player.seek(player.duration_secs());
            }
            TransportCommand::StepFrame(delta) => player.step_frame(delta),
            TransportCommand::SetVolume(_) => player.set_volume(volume),
            TransportCommand::ToggleMute => player.set_muted(muted),
            TransportCommand::ToggleLoop => player.set_looping(looping),
            TransportCommand::SetRate(_) => player.set_rate(rate),
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_modes_needing_both_images_are_flagged() {
        assert!(!CompareMode::SideBySide.needs_both_images());
        assert!(CompareMode::Swipe.needs_both_images());
        assert!(CompareMode::OnionSkin.needs_both_images());
        assert!(CompareMode::Difference.needs_both_images());
    }

    #[test]
    fn absent_reasons_have_copy() {
        assert_eq!(AbsentReason::NoPreviousVersion.title(), "No previous version");
        assert!(AbsentReason::Deleted.detail().contains("removes"));
    }

    fn state_with(old: MediaSideState, new: MediaSideState) -> MediaDiffState {
        MediaDiffState {
            kind: MediaKind::Image,
            generation: 1,
            file_path: "a.png".into(),
            old,
            new,
            image: ImageCompareState::default(),
            focused_side: MediaSide::New,
            linked_playback: false,
            show_info: false,
            volume: 1.0,
            muted: false,
            looping: false,
            rate: 1.0,
        }
    }

    #[test]
    fn effective_focus_falls_back_to_the_side_that_has_content() {
        let mut s = state_with(
            MediaSideState::Loading,
            MediaSideState::Absent(AbsentReason::Deleted),
        );
        s.focused_side = MediaSide::New;
        assert_eq!(s.effective_focus(), MediaSide::Old);
        let mut s = state_with(
            MediaSideState::Absent(AbsentReason::NoPreviousVersion),
            MediaSideState::Loading,
        );
        s.focused_side = MediaSide::Old;
        assert_eq!(s.effective_focus(), MediaSide::New);
    }

    #[test]
    fn view_for_respects_link_state() {
        let mut s = state_with(MediaSideState::Loading, MediaSideState::Loading);
        s.image.view = ImageView {
            scale: Some(2.0),
            center: (0.5, 0.5),
        };
        s.image.old_view = ImageView::fit();
        assert_eq!(s.view_for(MediaSide::Old), s.image.view);
        s.image.linked_views = false;
        assert_eq!(s.view_for(MediaSide::Old), ImageView::fit());
        assert_eq!(s.view_for(MediaSide::New), s.image.view);
    }
}
