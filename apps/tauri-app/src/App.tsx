import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, HardDrive, Settings2, AlertCircle } from "lucide-react";
import { useDevices } from "@/hooks/useDevices";
import { DeviceCard } from "@/components/device-card";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { toast } from "sonner";
import { useConfirm } from "@/hooks/use-confirm";
import type { DeviceInfo } from "@/types";
import "@/index.css";

import { MountSettingsDialog } from "@/components/mount-dialog";
import { SettingsDialog } from "@/components/settings-dialog";
import { useAtom } from "jotai";
import { tauriStoreAtom, appVersionAtom, fetchSteamState, steamStateAtom } from "./store";

function App() {
  const { devices, loading, error, refresh } = useDevices();
  const { confirm } = useConfirm();

  const [selectedDevice, setSelectedDevice] = useState<DeviceInfo | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [appVersion] = useAtom(appVersionAtom);
  const [tauriStore] = useAtom(tauriStoreAtom);
  const [steamState] = useAtom(steamStateAtom);

  // Initialize store once
  useEffect(() => {
    if (!tauriStore) {
      toast.error("Failed to load settings configuration");
    }
  }, []);

  const handleMountClick = (device: DeviceInfo) => {
    setSelectedDevice(device);
    setDialogOpen(true);
  };

  const handleUnmount = async (device: DeviceInfo) => {
    if (!device.mountpoint) return;

    // Use the global confirm dialog
    const confirmed = await confirm({
      title: "Confirm Unmount",
      description: `Are you sure you want to unmount ${device.label || device.name}? This might interrupt running applications.`,
      variant: "default",
    });

    if (!confirmed) return;

    try {
      await invoke("unmount_device", { mountPoint: device.mountpoint });
      toast.success("Successfully unmounted");
      refresh();
      fetchSteamState();
    } catch (e) {
      toast.error(`Unmount failed: ${e}`);
    }
  };

  const handleRepair = async (device: DeviceInfo) => {
    if (!device.uuid) return;

    try {
      await invoke("repair_dirty_volume", { uuid: device.uuid });
      toast.success("Repair successful! You can now mount the device.");
      refresh();
    } catch (e) {
      toast.error(`Repair failed: ${e}`);
    }
  };

  const handleDeconfigure = async (device: DeviceInfo) => {
    if (!device.uuid) return;

    // Use the global confirm dialog
    const confirmed = await confirm({
      title: "Confirm Deconfigure",
      description: `Are you sure you want to remove the fstab configuration for ${device.label || device.name}? This will remove the auto-mount entry but will not unmount the device if it's currently mounted.`,
      variant: "default",
    });

    if (!confirmed) return;

    try {
      await invoke("deconfigure_device", { uuid: device.uuid });
      toast.success("Device configuration removed successfully");
      refresh();
    } catch (e) {
      toast.error(`Deconfigure failed: ${e}`);
    }
  };

  const handleConfigureSteam = async (device: DeviceInfo) => {
    if (!device.mountpoint) return;

    try {
      await invoke("inject_steam_library", {
        config: {
          mountPoint: device.mountpoint,
          mode: "semi",
        }
      });

      // Steam command is fire-and-forget, so we ask user to confirm completion
      const confirmed = await confirm({
        title: "Configure Steam Library",
        description: "Steam Storage Manager has been opened. Please add the drive in Steam, then click Confirm here to refresh.",
        variant: "default",
        confirmText: "Refresh",
        cancelText: "Cancel"
      });

      if (confirmed) {
        refresh();
        fetchSteamState();
        toast.success("Refreshed device information");
      }
    } catch (e) {
      toast.error(`Failed to open Steam settings: ${e}`);
    }
  };

  const mountableDevices = useMemo(() => {
    return devices.filter(
      (d) => d.fstype === "ntfs" || d.fstype === "exfat"
    );
  }, [devices]);

  return (
    <div className="min-h-screen bg-background">
      <TooltipProvider>
        {/* Header */}
        <header className="border-b">
          <div className="container flex h-16 items-center justify-between">
            <div className="flex items-center gap-2">
              <HardDrive className="h-6 w-6" />
              <h1 className="text-xl font-bold">SteamOS Mount</h1>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="icon" onClick={() => { refresh(); fetchSteamState(); }}>
                <RefreshCw
                  className={`h-4 w-4 ${loading ? "animate-spin" : ""}`}
                />
              </Button>

              <Tooltip>
                <TooltipTrigger asChild>
                  <div className="inline-block">
                    <Button
                      variant={steamState?.isValid === false ? "outline" : "outline"}
                      size="icon"
                      onClick={() => setSettingsOpen(true)}
                      className={steamState?.isValid === false ? "border-yellow-500 text-yellow-600 hover:text-yellow-700 hover:bg-yellow-50 dark:text-yellow-400" : ""}
                    >
                      {steamState?.isValid === false ? (
                        <AlertCircle className="h-4 w-4" />
                      ) : (
                        <Settings2 className="h-4 w-4" />
                      )}
                    </Button>
                  </div>
                </TooltipTrigger>
                {steamState?.isValid === false && (
                  <TooltipContent>
                    <p className="font-semibold">Steam Configuration Issue</p>
                    <p className="text-xs">{steamState.error || "Library configuration invalid"}</p>
                  </TooltipContent>
                )}
              </Tooltip>
            </div>
          </div>
        </header>

        {/* Main Content */}
        <main className="container py-6">
          {error && (
            <div className="mb-4 p-4 bg-destructive/10 text-destructive rounded-lg">
              {error}
            </div>
          )}

          {loading && devices.length === 0 ? (
            <div className="flex items-center justify-center py-12">
              <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
            </div>
          ) : mountableDevices.length === 0 ? (
            <div className="text-center py-12 text-muted-foreground">
              <HardDrive className="h-12 w-12 mx-auto mb-4 opacity-50" />
              <p>No NTFS or exFAT devices found</p>
              <p className="text-sm mt-2">
                Connect an external drive or check your partitions
              </p>
            </div>
          ) : (
            <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(450px, 1fr))' }}>
              {mountableDevices.map((device) => (
                <DeviceCard
                  key={device.uuid || device.name}
                  device={device}
                  steamLibraries={steamState?.libraries}
                  onMount={handleMountClick}
                  onUnmount={handleUnmount}
                  onDeconfigure={handleDeconfigure}
                  onRepair={handleRepair}
                  onConfigureSteam={handleConfigureSteam}
                />
              ))}
            </div>
          )}
        </main>
      </TooltipProvider>

      <MountSettingsDialog
        device={selectedDevice}
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onSuccess={() => { refresh(); fetchSteamState(); }}
      />

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        onSaved={fetchSteamState}
        store={tauriStore}
      />

      {/* Footer */}
      <footer className="border-t mt-auto">
        <div className="container py-4 text-center text-sm text-muted-foreground">
          SteamOS Mount Tool v{appVersion} | Desktop Mode
        </div>
      </footer>
    </div>
  );
}

export default App;
