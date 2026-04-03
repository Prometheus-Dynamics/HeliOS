/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Access } from './Access';
import type { ControlId } from './ControlId';
import type { ControlKind } from './ControlKind';
import type { ControlMetadata } from './ControlMetadata';
import type { ControlValue } from './ControlValue';
/**
 * Simplified control metadata.
 *
 * # Example
 * ```rust
 * use styx_core::prelude::{Access, ControlId, ControlKind, ControlMeta, ControlMetadata, ControlValue};
 *
 * let meta = ControlMeta {
     * id: ControlId(1),
     * name: "gain".into(),
     * kind: ControlKind::Uint,
     * access: Access::ReadWrite,
     * min: ControlValue::Uint(0),
     * max: ControlValue::Uint(255),
     * default: ControlValue::Uint(16),
     * step: Some(ControlValue::Uint(1)),
     * menu: None,
     * metadata: ControlMetadata::default(),
     * };
     * assert!(meta.validate(&ControlValue::Uint(32)));
     * ```
     */
    export type ControlMeta = {
        /**
         * Access permissions.
         */
        access: Access;
        /**
         * Default value.
         */
        default: ControlValue;
        /**
         * Stable identifier.
         */
        id: ControlId;
        /**
         * Kind of control/value type.
         */
        kind: ControlKind;
        /**
         * Maximum accepted value.
         */
        max: ControlValue;
        /**
         * Optional enumerated menu entries (for menu controls).
         */
        menu?: any[] | null;
        /**
         * Optional metadata flags.
         */
        metadata?: ControlMetadata;
        /**
         * Minimum accepted value.
         */
        min: ControlValue;
        /**
         * Human-readable name.
         */
        name: string;
        step?: (null | ControlValue);
    };

