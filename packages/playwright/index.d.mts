import type {Fixtures,PlaywrightTestArgs,PlaywrightTestOptions,PlaywrightWorkerArgs,PlaywrightWorkerOptions,TestType} from "@playwright/test";
export {expect} from "@playwright/test";
export declare const mutationFixtures: Fixtures<{_errspecMutation:void},{_errspecNodeProcessObserver:void},PlaywrightTestArgs & PlaywrightTestOptions,PlaywrightWorkerArgs & PlaywrightWorkerOptions>;
export declare const test: TestType<PlaywrightTestArgs & PlaywrightTestOptions & {_errspecMutation:void},PlaywrightWorkerArgs & PlaywrightWorkerOptions & {_errspecNodeProcessObserver:void}>;
