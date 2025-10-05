export interface ManagedEntryInfo {
    mountPoint: string;
    options: string[];
    rawContent: string;
}

export interface SteamLibraryInfo {
    path: string;
    label: string;
}

// Device types matching the Rust backend
export interface DeviceInfo {
    name: string;
    path: string;
    label: string | null;
    uuid: string | null;
    partuuid: string | null;
    fstype: string;
    size: number;
    mountpoint: string | null;
    isMounted: boolean;
    isDirty: boolean;
    isOffline: boolean;
    managedEntry?: ManagedEntryInfo;
    fsSpec?: string;
    steamLibraries: SteamLibraryInfo[];
    rota?: boolean;
    removable?: boolean;
    transport?: string;
}

export interface MountConfig {
    uuid: string;
    mediaType: "flash" | "rotational";
    deviceType: "fixed" | "removable";
    deviceTimeoutSecs?: number;
    idleTimeoutSecs?: number;
    customOptions?: string;
    mountPoint: string;
    forceRootCreation: boolean;
    injectSteam: boolean;
    steamLibraryPath?: string;
}

export interface OptionMetadata {
    value: string;
    label: string;
    description: string;
    recommended: boolean;
}

export interface PresetConfigDto {
    mediaType: "flash" | "rotational";
    deviceType: "fixed" | "removable";
    deviceTimeoutSecs?: number;
    idleTimeoutSecs?: number;
}

export interface MountConfigSuggestion {
    defaultConfig: PresetConfigDto;
    connectionTypeOptions: OptionMetadata[];
    mediaTypeOptions: OptionMetadata[];
    deviceTimeoutDesc: string;
    idleTimeoutDesc: string;
}

export interface FstabPreview {
    options: string;
    fstabLine: string;
}

export interface SteamInjectionConfig {
    mountPoint: string;
    libraryPath?: string;
    steamVdfPath?: string;
    mode: "auto" | "semi" | "manual";
}

export interface SteamState {
    isValid: boolean;
    vdfPath: string;
    libraries: string[];
    error?: string;
}
