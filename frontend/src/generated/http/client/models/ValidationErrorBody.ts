/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ValidationIssue } from './ValidationIssue';
import type { ValidationWarning } from './ValidationWarning';
export type ValidationErrorBody = {
    code: string;
    error: string;
    issues: Array<ValidationIssue>;
    timestampMs: number;
    warnings?: Array<ValidationWarning>;
};

