export function target(platform?: string, arch?: string): string | null;
export function cacheDir(env?: Record<string, string | undefined>): string;
export function releaseUrl(name: string, env?: Record<string, string | undefined>): string;
export function binary(): Promise<string>;
