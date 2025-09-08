import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Store } from "@tauri-apps/plugin-store";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import { Save, RefreshCw, FolderOpen } from "lucide-react";

interface SettingsDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSaved?: () => void;
    store: Store | null;
}

const VDF_PATH_KEY = "steamLibraryVdfPath";

export function SettingsDialog({ open, onOpenChange, onSaved, store }: SettingsDialogProps) {
    const [vdfPath, setVdfPath] = useState("");
    const [loading, setLoading] = useState(false);

    // Initialize from store
    useEffect(() => {
        if (!store || !open) return;

        let active = true;

        const loadSettings = async () => {
            // Small delay to ensure dialog animation starts smoothly
            await new Promise(resolve => setTimeout(resolve, 50));
            if (!active) return;

            try {
                const val = await store.get<string>(VDF_PATH_KEY);
                if (active) {
                    if (val) {
                        setVdfPath(val);
                    } else if (vdfPath === "") { // Only if empty
                        // Only auto-detect if path is empty and not yet set
                        handleAutoDetect(false);
                    }
                }
            } catch (e) {
                console.error("Failed to load settings:", e);
                // Don't toast here to avoid spam/freeze if it loops
            }
        };

        loadSettings();

        return () => { active = false; };
    }, [open, store]);

    const handleAutoDetect = async (notify = true) => {
        setLoading(true);
        try {
            const path = await invoke<string>("detect_steam_library_vdf");
            setVdfPath(path);
            if (notify) toast.success("Detected Steam library VDF path");
        } catch (e) {
            if (notify) toast.error(`Failed to detect path: ${e}`);
        } finally {
            setLoading(false);
        }
    };

    const handleBrowse = async () => {
        try {
            const selected = await openDialog({
                multiple: false,
                filters: [{
                    name: 'Steam Library Config',
                    extensions: ['vdf']
                }],
                defaultPath: vdfPath || undefined,
            });

            if (selected && typeof selected === 'string') {
                if (!selected.endsWith("libraryfolders.vdf")) {
                    toast.warning("Selected file does not appear to be libraryfolders.vdf");
                }
                setVdfPath(selected);
            }
        } catch (e) {
            console.error("Failed to open dialog:", e);
        }
    };

    const handleSave = async () => {
        if (!store) return;
        setLoading(true);
        try {
            await store.set(VDF_PATH_KEY, vdfPath);
            await store.save();
            toast.success("Settings saved");
            onSaved?.();
            onOpenChange(false);
        } catch (e) {
            toast.error(`Failed to save settings: ${e}`);
        } finally {
            setLoading(false);
        }
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[500px]">
                <DialogHeader>
                    <DialogTitle>Settings</DialogTitle>
                    <DialogDescription>
                        Configure global application settings.
                    </DialogDescription>
                </DialogHeader>

                <div className="grid gap-4 py-4">
                    <div className="grid gap-2">
                        <Label htmlFor="vdf-path">Steam Library Config (libraryfolders.vdf)</Label>
                        <div className="flex gap-2">
                            <Input
                                id="vdf-path"
                                value={vdfPath}
                                onChange={(e) => setVdfPath(e.target.value)}
                                placeholder="/path/to/libraryfolders.vdf"
                                className="font-mono text-xs"
                            />
                            <Button variant="outline" size="icon" onClick={handleBrowse} title="Browse File">
                                <FolderOpen className="h-4 w-4" />
                            </Button>
                            <Button variant="outline" size="icon" onClick={() => handleAutoDetect(true)} disabled={loading} title="Auto Detect">
                                <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
                            </Button>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            Browse to select <code>libraryfolders.vdf</code>. Steam must be closed to modify it.
                        </p>
                    </div>
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
                    <Button onClick={handleSave} disabled={loading}>
                        <Save className="mr-2 h-4 w-4" />
                        Save
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
