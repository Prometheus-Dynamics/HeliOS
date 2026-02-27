/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import { ApiError } from './ApiError';
import type { ApiRequestOptions } from './ApiRequestOptions';
import type { ApiResult } from './ApiResult';
import { CancelablePromise } from './CancelablePromise';
import type { OnCancel } from './CancelablePromise';
import type { OpenAPIConfig } from './OpenAPI';
import { buildHttpCandidateUrls } from '../../../../api/httpCandidates';

type ExtendedApiRequestOptions = ApiRequestOptions & {
    timeout?: number;
    retryAttempts?: number;
};

const RETRYABLE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);
const RETRYABLE_STATUS = new Set([500, 502, 503, 504]);
const DEFAULT_TIMEOUT_MS = 15_000;
const MUTATION_TIMEOUT_MS = 30_000;
const MAX_RETRY_ATTEMPTS = 3;
const BASE_RETRY_DELAY_MS = 500;
const RETRY_JITTER_MS = 150;

export const isDefined = <T>(value: T | null | undefined): value is Exclude<T, null | undefined> => {
    return value !== undefined && value !== null;
};

export const isString = (value: any): value is string => {
    return typeof value === 'string';
};

export const isStringWithValue = (value: any): value is string => {
    return isString(value) && value !== '';
};

export const isBlob = (value: any): value is Blob => {
    return (
        typeof value === 'object' &&
        typeof value.type === 'string' &&
        typeof value.stream === 'function' &&
        typeof value.arrayBuffer === 'function' &&
        typeof value.constructor === 'function' &&
        typeof value.constructor.name === 'string' &&
        /^(Blob|File)$/.test(value.constructor.name) &&
        /^(Blob|File)$/.test(value[Symbol.toStringTag])
    );
};

export const isFormData = (value: any): value is FormData => {
    return value instanceof FormData;
};

export const base64 = (str: string): string => {
    try {
        return btoa(str);
    } catch (err) {
        // @ts-ignore
        return Buffer.from(str).toString('base64');
    }
};

export const getQueryString = (params: Record<string, any>): string => {
    const qs: string[] = [];

    const append = (key: string, value: any) => {
        qs.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`);
    };

    const process = (key: string, value: any) => {
        if (isDefined(value)) {
            if (Array.isArray(value)) {
                value.forEach(v => {
                    process(key, v);
                });
            } else if (typeof value === 'object') {
                Object.entries(value).forEach(([k, v]) => {
                    process(`${key}[${k}]`, v);
                });
            } else {
                append(key, value);
            }
        }
    };

    Object.entries(params).forEach(([key, value]) => {
        process(key, value);
    });

    if (qs.length > 0) {
        return `?${qs.join('&')}`;
    }

    return '';
};

const getUrl = (config: OpenAPIConfig, options: ApiRequestOptions): string => {
    const encoder = config.ENCODE_PATH || encodeURI;

    let path = options.url
        .replace('{api-version}', config.VERSION)
        .replace(/{(.*?)}/g, (substring: string, group: string) => {
            if (options.path?.hasOwnProperty(group)) {
                return encoder(String(options.path[group]));
            }
            return substring;
        });

    // The device image serves the SPA at `/` and reverse-proxies the API under `/v1/*`.
    // The generated bindings target the API root (e.g. `/streams`), so transparently
    // prefix `/v1` unless the configured base URL already includes it.
    const normalizedBase = String(config.BASE ?? '').replace(/\/+$/, '');
    const baseAlreadyHasV1 = (() => {
        if (!normalizedBase) return false;
        if (/(^|\/)v1$/.test(normalizedBase)) return true;
        try {
            const parsed = new URL(normalizedBase);
            return parsed.pathname.replace(/\/+$/, '').endsWith('/v1');
        } catch {
            return false;
        }
    })();
    if (!baseAlreadyHasV1 && !path.startsWith('/v1/') && path !== '/v1' && !path.startsWith('/openapi.json') && !path.startsWith('/asyncapi.json')) {
        path = `/v1${path.startsWith('/') ? '' : '/'}${path}`;
    }

    const url = `${config.BASE}${path}`;
    if (options.query) {
        return `${url}${getQueryString(options.query)}`;
    }
    return url;
};

export const getFormData = (options: ApiRequestOptions): FormData | undefined => {
    if (options.formData) {
        const formData = new FormData();

        const process = (key: string, value: any) => {
            if (isString(value) || isBlob(value)) {
                formData.append(key, value);
            } else {
                formData.append(key, JSON.stringify(value));
            }
        };

        Object.entries(options.formData)
            .filter(([_, value]) => isDefined(value))
            .forEach(([key, value]) => {
                if (Array.isArray(value)) {
                    value.forEach(v => process(key, v));
                } else {
                    process(key, value);
                }
            });

        return formData;
    }
    return undefined;
};

type Resolver<T> = (options: ApiRequestOptions) => Promise<T>;

export const resolve = async <T>(options: ApiRequestOptions, resolver?: T | Resolver<T>): Promise<T | undefined> => {
    if (typeof resolver === 'function') {
        return (resolver as Resolver<T>)(options);
    }
    return resolver;
};

export const getHeaders = async (config: OpenAPIConfig, options: ApiRequestOptions): Promise<Headers> => {
    const [token, username, password, additionalHeaders] = await Promise.all([
        resolve(options, config.TOKEN),
        resolve(options, config.USERNAME),
        resolve(options, config.PASSWORD),
        resolve(options, config.HEADERS),
    ]);

    const headers = Object.entries({
        Accept: 'application/json',
        ...additionalHeaders,
        ...options.headers,
    })
        .filter(([_, value]) => isDefined(value))
        .reduce((headers, [key, value]) => ({
            ...headers,
            [key]: String(value),
        }), {} as Record<string, string>);

    if (isStringWithValue(token)) {
        headers['Authorization'] = `Bearer ${token}`;
    }

    if (isStringWithValue(username) && isStringWithValue(password)) {
        const credentials = base64(`${username}:${password}`);
        headers['Authorization'] = `Basic ${credentials}`;
    }

    if (options.body !== undefined) {
        if (options.mediaType) {
            headers['Content-Type'] = options.mediaType;
        } else if (isBlob(options.body)) {
            headers['Content-Type'] = options.body.type || 'application/octet-stream';
        } else if (isString(options.body)) {
            headers['Content-Type'] = 'text/plain';
        } else if (!isFormData(options.body)) {
            headers['Content-Type'] = 'application/json';
        }
    }

    return new Headers(headers);
};

export const getRequestBody = (options: ApiRequestOptions): any => {
    if (options.body !== undefined) {
        if (options.mediaType?.includes('/json')) {
            return JSON.stringify(options.body)
        } else if (isString(options.body) || isBlob(options.body) || isFormData(options.body)) {
            return options.body;
        } else {
            return JSON.stringify(options.body);
        }
    }
    return undefined;
};

export const sendRequest = async (
    config: OpenAPIConfig,
    options: ApiRequestOptions,
    url: string,
    body: any,
    formData: FormData | undefined,
    headers: Headers,
    timeoutMs: number | undefined,
    onCancel: OnCancel
): Promise<Response> => {
    const controller = new AbortController();
    let timeoutHandle: ReturnType<typeof setTimeout> | undefined;

    const request: RequestInit = {
        headers,
        body: body ?? formData,
        method: options.method,
        signal: controller.signal,
    };

    if (config.WITH_CREDENTIALS) {
        request.credentials = config.CREDENTIALS;
    }

    if (Number.isFinite(timeoutMs) && timeoutMs && timeoutMs > 0) {
        timeoutHandle = setTimeout(() => controller.abort(), timeoutMs);
    }

    onCancel(() => {
        if (!controller.signal.aborted) {
            controller.abort();
        }
        if (timeoutHandle) {
            clearTimeout(timeoutHandle);
        }
    });

    try {
        return await fetch(url, request);
    } finally {
        if (timeoutHandle) {
            clearTimeout(timeoutHandle);
        }
    }
};

export const getResponseHeader = (response: Response, responseHeader?: string): string | undefined => {
    if (responseHeader) {
        const content = response.headers.get(responseHeader);
        if (isString(content)) {
            return content;
        }
    }
    return undefined;
};

export const getResponseBody = async (response: Response): Promise<any> => {
    if (response.status !== 204) {
        try {
            const contentType = response.headers.get('Content-Type');
            if (contentType) {
                const jsonTypes = ['application/json', 'application/problem+json']
                const isJSON = jsonTypes.some(type => contentType.toLowerCase().startsWith(type));
                if (isJSON) {
                    return await response.json();
                } else {
                    return await response.text();
                }
            }
        } catch (error) {
            console.error(error);
        }
    }
    return undefined;
};

export const catchErrorCodes = (options: ApiRequestOptions, result: ApiResult): void => {
    const errors: Record<number, string> = {
        400: 'Bad Request',
        401: 'Unauthorized',
        403: 'Forbidden',
        404: 'Not Found',
        500: 'Internal Server Error',
        502: 'Bad Gateway',
        503: 'Service Unavailable',
        ...options.errors,
    }

    const error = errors[result.status];
    if (error) {
        throw new ApiError(options, result, error);
    }

    if (!result.ok) {
        const errorStatus = result.status ?? 'unknown';
        const errorStatusText = result.statusText ?? 'unknown';
        const errorBody = (() => {
            try {
                return JSON.stringify(result.body, null, 2);
            } catch (e) {
                return undefined;
            }
        })();

        throw new ApiError(options, result,
            `Generic Error: status: ${errorStatus}; status text: ${errorStatusText}; body: ${errorBody}`
        );
    }
};

const sleep = (ms: number): Promise<void> => new Promise(resolve => setTimeout(resolve, ms));

const describeUnknownError = (error: unknown): string => {
    if (error instanceof Error && isStringWithValue(error.message)) {
        return error.message;
    }
    if (typeof error === 'string' && error.trim().length > 0) {
        return error.trim();
    }
    return 'Request failed';
};

const isRetryableNetworkError = (error: unknown): boolean => {
    if (typeof DOMException !== 'undefined' && error instanceof DOMException && error.name === 'AbortError') {
        return true;
    }
    if (error instanceof Error) {
        const message = error.message.toLowerCase();
        return message.includes('network') || message.includes('fetch') || message.includes('time') || message.includes('abort');
    }
    return false;
};

const shouldRetryStatus = (status?: number | null): boolean => {
    if (status == null) {
        return false;
    }
    return RETRYABLE_STATUS.has(status);
};

const nextBackoffDelay = (attempt: number): number => {
    const exponential = BASE_RETRY_DELAY_MS * Math.pow(2, attempt);
    const capped = Math.min(exponential, 4_000);
    const jitter = Math.random() * RETRY_JITTER_MS;
    return capped + jitter;
};

/**
 * Request method
 * @param config The OpenAPI configuration object
 * @param options The request options from the service
 * @returns CancelablePromise<T>
 * @throws ApiError
 */
export const request = <T>(config: OpenAPIConfig, options: ApiRequestOptions): CancelablePromise<T> => {
    return new CancelablePromise(async (resolve, reject, onCancel) => {
        const url = getUrl(config, options);
        const formData = getFormData(options);
        const body = getRequestBody(options);
        const headers = await getHeaders(config, options);
        const extended = options as ExtendedApiRequestOptions;
        const method = options.method?.toUpperCase() ?? 'GET';
        const retryEligible = RETRYABLE_METHODS.has(method);
        const maxAttempts = Math.max(1, extended.retryAttempts ?? (retryEligible ? MAX_RETRY_ATTEMPTS : 1));
        const timeoutMs = extended.timeout ?? (retryEligible ? DEFAULT_TIMEOUT_MS : MUTATION_TIMEOUT_MS);

        let attempt = 0;
        attemptLoop: while (attempt < maxAttempts) {
            if (onCancel.isCancelled) {
                return reject(new Error('Request cancelled'));
            }

            const candidates = retryEligible ? buildHttpCandidateUrls(url) : [url];
            let lastTransportError: unknown = null;

            for (let index = 0; index < candidates.length; index += 1) {
                const candidateUrl = candidates[index] ?? url;
                const hasMoreCandidates = index + 1 < candidates.length;
                try {
                    const response = await sendRequest(config, options, candidateUrl, body, formData, headers, timeoutMs, onCancel);
                    const responseBody = await getResponseBody(response);
                    const responseHeader = getResponseHeader(response, options.responseHeader);

                    const result: ApiResult = {
                        url: candidateUrl,
                        ok: response.ok,
                        status: response.status,
                        statusText: response.statusText,
                        body: responseHeader ?? responseBody,
                    };

                    const status = result.status ?? 0;
                    const retryableStatus = !result.ok && shouldRetryStatus(status);

                    if (retryableStatus && hasMoreCandidates) {
                        continue;
                    }
                    if (retryableStatus && attempt + 1 < maxAttempts) {
                        attempt += 1;
                        await sleep(nextBackoffDelay(attempt - 1));
                        continue attemptLoop;
                    }

                    try {
                        catchErrorCodes(options, result);
                        resolve(result.body as T);
                        return;
                    } catch (error) {
                        reject(error);
                        return;
                    }
                } catch (error) {
                    if (onCancel.isCancelled) {
                        return reject(error);
                    }
                    const retryableTransport = retryEligible || isRetryableNetworkError(error);
                    lastTransportError = error;
                    if (hasMoreCandidates && retryableTransport) {
                        continue;
                    }
                    const retryable = attempt + 1 < maxAttempts && retryableTransport;
                    if (!retryable) {
                        return reject(error);
                    }
                    attempt += 1;
                    await sleep(nextBackoffDelay(attempt - 1));
                    continue attemptLoop;
                }
            }

            attempt += 1;
            if (attempt >= maxAttempts) {
                reject(lastTransportError ?? new Error('Request retry attempts exhausted'));
                return;
            }
            await sleep(nextBackoffDelay(attempt - 1));
        }
    });
};
