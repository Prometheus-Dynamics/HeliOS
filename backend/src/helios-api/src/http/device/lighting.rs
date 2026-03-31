mod animations;
mod config;
mod runtime;
mod templates;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use animations::{
    __path_delete_lighting_animation, __path_list_lighting_animations, __path_save_lighting_animation, delete_lighting_animation, list_lighting_animations, save_lighting_animation,
};
pub(crate) use config::{__path_lighting_config, __path_reset_lighting_config, __path_update_lighting_config, lighting_config, reset_lighting_config, update_lighting_config};
pub(crate) use runtime::{__path_lighting_command, __path_lighting_state, lighting_command, lighting_state};
pub(crate) use templates::{__path_fetch_lighting_template, __path_list_lighting_templates, fetch_lighting_template, list_lighting_templates};
pub use types::{
    LightingAnimationEntryResponse, LightingAnimationListResponse, LightingAnimationPayload, LightingAnimationSaveRequest, LightingAnimationTemplateDocument, LightingAnimationTemplateSummary,
    LightingColorPayload, LightingCommandRequest, LightingConfigRequest, LightingFramePayload, LightingRuntimeStatePayload, LightingTimelinePayload,
};
