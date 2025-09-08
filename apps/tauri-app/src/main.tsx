import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Providers } from "@/components/providers";
import { getVersion } from "@tauri-apps/api/app";
import { Store as TauriStore } from "@tauri-apps/plugin-store";
import { appStore, appVersionAtom, tauriStoreAtom } from "./store";

async function bootstrap() {
  const [version, store] = await Promise.all([
    getVersion().catch(() => "Unknown"),
    TauriStore.load("settings.json").catch(() => {
      console.error("Failed to load store");
      return null;
    })
  ]);

  appStore.set(appVersionAtom, version);
  appStore.set(tauriStoreAtom, store);

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <Providers appStore={appStore}>
        <App />
      </Providers>
    </React.StrictMode>
  );
}

bootstrap();
