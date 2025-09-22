export interface ManagedEntryInfo {
    mountPoint: string;
    options: string[];
    rawContent: string;
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
    managedEntry?: ManagedEntryInfo;
}

export interface MountConfig {
    uuid: string;
    preset: "ssd" | "portable" | "custom";
    mediaType: "flash" | "rotational";
    deviceType: "fixed" | "removable";
    customOptions?: string;
    mountPoint: string;
    forceRootCreation: boolean;
    injectSteam: boolean;
    steamLibraryPath?: string;
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

export interface PresetInfo {
    id: string;
    name: string;
    description: string;
    optionsPreview: string;
}
