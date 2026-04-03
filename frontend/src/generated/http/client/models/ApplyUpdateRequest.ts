/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApplyArtifactKind } from './ApplyArtifactKind';
export type ApplyUpdateRequest = {
    artifact_kind?: (null | ApplyArtifactKind);
    checksum?: string | null;
    delete_image_after_apply?: boolean;
    image_url?: string | null;
    requested_by?: string | null;
    size_bytes?: number | null;
    update_id?: string | null;
};

