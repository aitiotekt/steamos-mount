import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useConfirm } from "@/hooks/use-confirm";
import { toast } from "sonner";
import { Info } from "lucide-react";
import type { DeviceInfo, MountConfig } from "@/types";

interface MountSettingsDialogProps {
    device: DeviceInfo | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSuccess: () => void;
}

export function MountSettingsDialog({ device, open, onOpenChange, onSuccess }: MountSettingsDialogProps) {
    const [mountPoint, setMountPoint] = useState("");
    const [loading, setLoading] = useState(false);
    const [preset, setPreset] = useState<"ssd" | "portable">("ssd");
    const { confirm } = useConfirm();

    // Load default mount point when dialog opens
    useEffect(() => {
        if (open && device?.uuid) {
            invoke<string>("get_default_mount_point", { uuid: device.uuid })
                .then(setMountPoint)
                .catch(console.error);
        }
    }, [open, device]);

    const handleMount = async (forceRoot: boolean = false) => {
        if (!device?.uuid) return;

        if (!mountPoint) {
            toast.error("Mount point is required. Please enter a valid path.");
            return;
        }

        setLoading(true);
        try {
            const config: MountConfig = {
                uuid: device.uuid,
                preset,
                mediaType: "flash", // Simplified for now, could be dynamic
                deviceType: "fixed", // Simplified
                mountPoint,
                forceRootCreation: forceRoot,
                injectSteam: false,
            };

            await invoke("mount_device", { config });

            toast.success(`Successfully mounted to ${mountPoint}`);
            onSuccess();
            onOpenChange(false);
        } catch (e) {
            const errorMessage = String(e || "");

            // Check for permission denied error
            if (errorMessage.includes("permission denied creating mount point")) {
                const confirmed = await confirm({
                    title: "Permission Denied",
                    description: `Failed to create mount point "${mountPoint}" with current permissions. Do you want to try creating it with root privileges (sudo/pkexec)?`,
                    variant: "default",
                });

                if (confirmed) {
                    handleMount(true); // Retry with forceRoot
                }
                return;
            }

            toast.error(`Mount failed: ${errorMessage}`);
        } finally {
            setLoading(false);
        }
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-[500px]">
                <DialogHeader>
                    <DialogTitle>Mount Configuration</DialogTitle>
                    <DialogDescription>
                        Configure mount options for {device?.label || device?.name || "device"}
                    </DialogDescription>
                </DialogHeader>

                <div className="grid gap-4 py-4">
                    <div className="grid gap-2">
                        <Label>Preset</Label>
                        <Select value={preset} onValueChange={(v: any) => setPreset(v)}>
                            <SelectTrigger>
                                <SelectValue placeholder="Select preset" />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="ssd">Internal SSD (High Performance)</SelectItem>
                                <SelectItem value="portable">Portable Drive (Compatibility)</SelectItem>
                            </SelectContent>
                        </Select>
                    </div>

                    <div className="grid gap-2">
                        <Label>Mount Point</Label>
                        <Input
                            value={mountPoint}
                            onChange={(e) => setMountPoint(e.target.value)}
                            placeholder="/home/deck/Drives/..."
                        />
                        <div className="rounded-md bg-muted p-3 text-sm flex gap-2 items-start text-muted-foreground">
                            <Info className="h-4 w-4 mt-0.5 shrink-0" />
                            <div>
                                <p className="font-medium text-foreground">Recommended Path</p>
                                Default path allows mounting without root password and keeps your drives organized in your home folder.
                            </div>
                        </div>
                    </div>
                </div>

                <DialogFooter>
                    <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
                    <Button onClick={() => handleMount(false)} disabled={loading}>
                        {loading ? "Mounting..." : "Mount"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
