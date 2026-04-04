#![forbid(unsafe_code)]

mod full;
mod hybrid;
mod light;
pub(crate) mod model;
pub(crate) mod normalize;
pub(crate) mod registry;
mod selective;
pub(crate) mod specialize;
pub(crate) mod validate;

#[cfg(test)]
mod tests;

pub use full::{
    collect_top_level_facts, collect_top_level_facts_shared,
    collect_top_level_facts_shared_with_registry, collect_top_level_facts_with_registry,
};
pub use hybrid::{
    collect_top_level_facts_hybrid, collect_top_level_facts_hybrid_report,
    collect_top_level_facts_hybrid_report_shared,
    collect_top_level_facts_hybrid_report_shared_with_decider,
    collect_top_level_facts_hybrid_report_shared_with_registry,
    collect_top_level_facts_hybrid_report_shared_with_registry_and_decider,
    collect_top_level_facts_hybrid_report_with_decider,
    collect_top_level_facts_hybrid_report_with_registry,
    collect_top_level_facts_hybrid_report_with_registry_and_decider,
    collect_top_level_facts_hybrid_shared, collect_top_level_facts_hybrid_shared_with_decider,
    collect_top_level_facts_hybrid_shared_with_registry,
    collect_top_level_facts_hybrid_shared_with_registry_and_decider,
    collect_top_level_facts_hybrid_shared_with_registry_and_options,
    collect_top_level_facts_hybrid_with_decider, collect_top_level_facts_hybrid_with_registry,
    collect_top_level_facts_hybrid_with_registry_and_decider,
    collect_top_level_facts_hybrid_with_registry_and_options,
    promote_light_top_level_command_shared_with_registry,
    promote_light_top_level_command_shared_with_registry_and_decider,
    promote_light_top_level_command_shared_with_registry_and_options,
    promote_light_top_level_command_with_registry,
    promote_light_top_level_command_with_registry_and_decider,
    promote_light_top_level_command_with_registry_and_options,
};
pub use light::{
    collect_top_level_facts_light, collect_top_level_facts_light_shared,
    collect_top_level_facts_light_shared_with_registry,
    collect_top_level_facts_light_with_registry,
};
pub use model::*;
pub use registry::MayaCommandRegistry;
pub use selective::{
    collect_selective_top_level_bytes_with_encoding_and_options_and_sink,
    collect_selective_top_level_bytes_with_encoding_and_sink,
    collect_selective_top_level_bytes_with_options_and_sink,
    collect_selective_top_level_bytes_with_sink,
    collect_selective_top_level_file_with_encoding_and_options_and_sink,
    collect_selective_top_level_file_with_encoding_and_sink,
    collect_selective_top_level_file_with_light_options_and_sink,
    collect_selective_top_level_file_with_options_and_sink,
    collect_selective_top_level_file_with_sink,
    collect_selective_top_level_source_with_options_and_sink,
    collect_selective_top_level_source_with_sink,
};
