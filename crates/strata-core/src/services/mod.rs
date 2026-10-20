// SPDX-License-Identifier: MIT

pub mod actions;
pub mod camera_preview;
pub mod diagnostics;
mod document;
pub mod document_media;
pub mod docx;
pub mod file_providers;
mod file_source;
mod install_source;
pub mod integration_catalog;
pub mod jobs;
mod listeners;
mod mime_type;
pub mod model_preview;
mod native_fs;
mod navigation_history;
mod operations;
pub mod package_manager;
mod path_match;
mod preview;
mod release_channel;
pub mod rtf;
mod search;
pub mod table;
mod transfer_action;

pub use actions::{
    ActionAvailability, ActionHandle, ActionLoadFailure, ActionProgram, ActionRegistry,
    ActionScript, ActionWriteRequest, MatchedAction,
};
pub use document::{
    DocumentLayout, DocumentListChildKind, DocumentMedia, DocumentSpan, DocumentSpanStyle,
    DocumentTableCellLayout, DocumentUnit, DocumentUnitKind, document_kind, has_web_scheme,
    layout_document, parse_document,
};
pub use file_source::{
    DirectoryChange, DirectoryEvent, DirectoryRequest, FileSource, LoadHandle, LocationIdentity,
    LocationValidationError, MetadataOutcome, MetadataRequest, MetadataUpdate, RequestId,
    UriCredentials, backend_unavailable_message, sanitize_uri_credentials,
    validate_uri_credentials,
};
pub use file_source::{
    INVALID_NAME_REASON, error_detail, gio_error_detail, gio_error_message, io_error_detail,
    io_error_message, sanitize_failure_message,
};
pub use install_source::InstallSource;
pub use install_source::installed_executable;
pub use jobs::{
    ActionEventSink, ActionRunEvent, ActionRunRequest, ActionRunner, CancelHandle,
    InvocationSource, JobId, JobRequest, JobService, JobSnapshot, JobStatus, ScriptProgress,
    expand_command_arguments,
};
pub use listeners::ListenerGuard;
pub use mime_type::{
    BROKEN_LINK_TYPE_NAME, EntryType, FOLDER_TYPE_NAME, OTHER_TYPE_NAME, entry_type,
    mime_description_for_name,
};
pub use model_preview::ModelRender;
pub use model_preview::{ModelEncoding, ModelFormat, ModelMesh, ModelPalette, ModelView};
pub use native_fs::{is_hidden_name, native_hidden_names, native_kind, symlink_kind};
pub use navigation_history::NavigationHistory;
pub use operations::{
    ArchiveFormat, CancelledOperation, CompressRequest, CreateDirectoryRequest, CreateFileRequest,
    DeleteRequest, ExtractRequest, KeptCause, KeptItem, MoveRecord, NameMode, OperationEvent,
    OperationProvider, OperationRequestId, PasswordFailure, PasteItem, PasteRequest,
    PlacedSnapshot, RenameCause, RenameRecord, RenameRequest, RenamedItem, ReplacedOriginal,
    RestoreRequest, RestoreSource, RestoreTrashItem, ShownName, SkippedItem, SpecialKind,
    TransferConflict, TransferReport, TrashedOriginal, UndoCopyRequest, UndoMergeRequest,
    UndoMoveItem, UndoMoveRequest, UndoRenameRequest, validate_basename,
};
pub use path_match::{PathMatcher, PathQuery};
pub use preview::{
    ArchiveDirectory, ArchiveFileEntry, ArchiveNode, ArchivePreviewTree, MediaPreviewSize,
    ModelPreviewStage, PdfTextLayer, Preview, PreviewContent, PreviewEvent, PreviewProvider,
    PreviewRequest, PreviewRequestId, SandboxedMedia, SecretString, archive_preview_tree,
};
pub use preview::{
    INCORRECT_ARCHIVE_PASSWORD, archive_preview_format, bytes_support_named_family, content_family,
    guess_content_type, has_plain_text_extension, is_extensionless_dotfile, is_image_path,
    is_media_path, is_model, is_non_executable_extensionless_dotfile, looks_like_text,
    normalize_preview_text, read_head_sample, reconcile_with_sample, split_archive_name,
    supports_remote_video,
};
pub use release_channel::{BuildKind, Version};
pub use search::{
    RESULT_LIMIT as SEARCH_RESULT_LIMIT, RenameScope, rebase_search_indexes,
    refresh_search_indexes_for_directory, refresh_search_indexes_for_rename,
};
pub use search::{
    RefusedFolders, SearchCoverage, SearchEvent, SearchExclusions, SearchHandle, SearchItem,
    SearchLimits, filter_name_matches, filter_query_allows_typos, fold_for_search, index_filter,
    index_folder_paths, index_paths, index_trees_with_exclusions,
};
pub use transfer_action::{
    CrossVolumeDropStrategy, DropActionInput, DropCommit, DropOverride, TransferKind,
    VolumeIdentity, VolumeRelation, drop_commit, transferable_drop_sources, volume_relation,
};
