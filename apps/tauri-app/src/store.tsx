import { atom, createStore } from "jotai";
import { Store as TauriStore } from '@tauri-apps/plugin-store';
import type { SteamState } from "./types";
import { invoke } from "@tauri-apps/api/core";

const appVersionAtom = atom<string>("");
const tauriStoreAtom = atom<TauriStore | null>(null);
const steamStateAtom = atom<SteamState | null>(null);
const appStore = createStore();

async function fetchSteamState() {
    const tauriStore = appStore.get(tauriStoreAtom);
    if (!tauriStore) {
        return;
    }
    try {
        const path = await tauriStore.get<string>("steamLibraryVdfPath");
        const state = await invoke<SteamState>("get_steam_state", { steamVdfPath: path });
        appStore.set(steamStateAtom, state);
    } catch (e) {
        console.error("Failed to fetch steam state", e);
    }
}

appStore.sub(tauriStoreAtom, fetchSteamState)

export {
    appStore,
    appVersionAtom,
    tauriStoreAtom,
    steamStateAtom,
    fetchSteamState
}