/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type RecordingSource = ({
    kind: 'multiplex';
} | {
    kind: 'raw';
} | {
    kind: 'pipeline';
    output_key?: string | null;
    pipeline_id?: string | null;
});

