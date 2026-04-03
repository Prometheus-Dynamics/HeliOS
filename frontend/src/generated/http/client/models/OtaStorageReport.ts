/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { OtaDirectoryStorageReport } from './OtaDirectoryStorageReport';
import type { OtaUploadStorageReport } from './OtaUploadStorageReport';
export type OtaStorageReport = {
    cache: OtaDirectoryStorageReport;
    frontend_releases: OtaDirectoryStorageReport;
    service_releases: OtaDirectoryStorageReport;
    uploads: OtaUploadStorageReport;
    work: OtaDirectoryStorageReport;
};

