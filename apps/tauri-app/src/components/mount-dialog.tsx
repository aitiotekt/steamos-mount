import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { useConfirm } from "@/hooks/use-confirm";
import { toast } from "sonner";
import { Info, CheckCircle2, Circle, HardDrive, Usb } from "lucide-react";
import type { DeviceInfo, MountConfig, FstabPreview, MountConfigSuggestion } from "@/types";
import { cn } from "@/lib/utils";

interface MountSettingsDialogProps {
    device: DeviceInfo | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSuccess: () => void;
}

export function MountSettingsDialog({ device, open, onOpenChange, onSuccess }: MountSettingsDialogProps) {
    const [mountPoint, setMountPoint] = useState("");
    const [loading, setLoading] = useState(false);
    const [suggestion, setSuggestion] = useState<MountConfigSuggestion | null>(null);

    // Orthogonal Options
    const [connectionType, setConnectionType] = useState<"fixed" | "removable">("fixed");
    const [mediaType, setMediaType] = useState<"flash" | "rotational">("flash");

    // Timeouts
    const [deviceTimeout, setDeviceTimeout] = useState(3);
    const [idleTimeout, setIdleTimeout] = useState(60);

    const [preview, setPreview] = useState<FstabPreview | null>(null);
    const { confirm } = useConfirm();

    // Fetch suggestion and defaults when dialog opens
    useEffect(() => {
        if (open && device) {
            // Get suggestion from backend
            invoke<MountConfigSuggestion>("get_mount_config_suggestion", { uuid: device.uuid })
                .then((sugg) => {
                    setSuggestion(sugg);

                    // Apply defaults from suggestion
                    setConnectionType(sugg.defaultConfig.deviceType);
                    setMediaType(sugg.defaultConfig.mediaType);
                    setDeviceTimeout(sugg.defaultConfig.deviceTimeoutSecs || 0);
                    setIdleTimeout(sugg.defaultConfig.idleTimeoutSecs || 0);
                })
                .catch(err => {
                    console.error("Failed to get suggestion:", err);
                    // Fallback defaults if API fails
                    setConnectionType("fixed");
                    setMediaType("flash");
                });

            // Get mount point default
            invoke<string>("get_default_mount_point", { uuid: device.uuid })
                .then(setMountPoint)
                .catch(console.error);
        } else {
            setSuggestion(null);
        }
    }, [open, device]);

    // Live Preview with Debounce
    useEffect(() => {
        if (!open || !device?.uuid || !mountPoint) return;

        const timer = setTimeout(() => {
            const config: MountConfig = {
                uuid: device.uuid!,
                mediaType,
                deviceType: connectionType,
                deviceTimeoutSecs: connectionType === "fixed" ? deviceTimeout : undefined,
                idleTimeoutSecs: connectionType === "removable" ? idleTimeout : undefined,
                mountPoint,
                forceRootCreation: false,
                injectSteam: false,
            };

            invoke<FstabPreview>("preview_mount_options", { config })
                .then(setPreview)
                .catch(err => console.error("Preview failed:", err));
        }, 300);

        return () => clearTimeout(timer);
    }, [open, device, mountPoint, connectionType, mediaType, deviceTimeout, idleTimeout]);

    const handleMount = async (forceRoot: boolean = false) => {
        if (!device?.uuid) return;

        if (!mountPoint) {
            toast.error("Mount point is required.");
            return;
        }

        setLoading(true);
        try {
            const config: MountConfig = {
                uuid: device.uuid,
                mediaType,
                deviceType: connectionType,
                deviceTimeoutSecs: connectionType === "fixed" ? deviceTimeout : undefined,
                idleTimeoutSecs: connectionType === "removable" ? idleTimeout : undefined,
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
            if (errorMessage.includes("permission denied creating mount point")) {
                const confirmed = await confirm({
                    title: "Permission Denied",
                    description: `Failed to create mount point "${mountPoint}" with current permissions. Try with root privileges?`,
                    variant: "default",
                });

                if (confirmed) {
                    handleMount(true);
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
            <DialogContent className="sm:max-w-[480px] max-h-[85vh] flex flex-col gap-0">
                <DialogHeader className="flex-shrink-0 pb-4">
                    <div className="flex items-center justify-between">
                        <DialogTitle>Mount Configuration</DialogTitle>
                        <div className="flex gap-1">
                            {device?.transport && (
                                <Badge variant="outline" className="px-1.5 h-5 text-[10px] font-mono uppercase">
                                    {device.transport}
                                </Badge>
                            )}
                            {device?.rota !== undefined && (
                                <Badge variant="secondary" className="px-1.5 h-5 text-[10px]">
                                    {device.rota ? "ROTA" : "FLASH"}
                                </Badge>
                            )}
                            {device?.removable && (
                                <Badge variant="outline" className="px-1.5 h-5 text-[10px]">
                                    Removable
                                </Badge>
                            )}
                            <Badge variant="outline" className="px-1.5 h-5 text-[10px] font-mono">{device?.fstype?.toUpperCase()}</Badge>
                        </div>
                    </div>
                    <DialogDescription>
                        Configure options for {device?.label || device?.name}
                    </DialogDescription>
                </DialogHeader>

                <div className="flex-1 overflow-y-auto min-h-0 pr-2 -mr-2">
                    {/* Render content only when suggestion is loaded to avoid layout shifts */}
                    {suggestion ? (
                        <div className="flex flex-col gap-4 py-2">
                            {/* Connection Type */}
                            <div className="space-y-2">
                                <Label>Connection Type</Label>
                                <div className="grid grid-cols-2 gap-3">
                                    {suggestion.connectionTypeOptions.map((opt) => (
                                        <div
                                            key={opt.value}
                                            className={cn(
                                                "flex flex-col gap-1.5 rounded-md border-2 p-3 cursor-pointer transition-colors relative",
                                                connectionType === opt.value
                                                    ? "border-primary bg-primary/10"
                                                    : "border-muted hover:bg-muted/50"
                                            )}
                                            onClick={() => setConnectionType(opt.value as "fixed" | "removable")}
                                        >
                                            <div className="flex items-center justify-between">
                                                <div className="flex items-center gap-2 font-medium text-sm">
                                                    {opt.value === "fixed" ? <HardDrive className="h-4 w-4" /> : <Usb className="h-4 w-4" />}
                                                    {opt.label}
                                                </div>
                                                {connectionType === opt.value ? (
                                                    <CheckCircle2 className="h-4 w-4 text-primary" />
                                                ) : (
                                                    <Circle className="h-4 w-4 text-muted-foreground" />
                                                )}
                                            </div>
                                            <p className="text-[10px] text-muted-foreground leading-tight">{opt.description}</p>
                                            {/* Badge for Recommended - Only show if it matches the originally recommended one */}
                                            {opt.recommended && (
                                                <Badge variant="secondary" className="mt-1 w-fit text-[10px] h-4 px-1 py-0 pointer-events-none">
                                                    Recommended
                                                </Badge>
                                            )}
                                        </div>
                                    ))}
                                </div>
                            </div>

                            {/* Storage Media */}
                            <div className="space-y-2">
                                <Label>Storage Media</Label>
                                <div className="grid grid-cols-2 gap-3">
                                    {suggestion.mediaTypeOptions.map((opt) => (
                                        <div
                                            key={opt.value}
                                            className={cn(
                                                "flex flex-col gap-1 rounded-md border p-2 cursor-pointer transition-colors",
                                                mediaType === opt.value
                                                    ? "border-primary bg-primary/10"
                                                    : "border-muted hover:bg-muted/50"
                                            )}
                                            onClick={() => setMediaType(opt.value as "flash" | "rotational")}
                                        >
                                            <div className="flex items-center justify-between">
                                                <span className="text-sm font-medium">{opt.label}</span>
                                                {mediaType === opt.value && <CheckCircle2 className="h-3 w-3 text-primary" />}
                                            </div>
                                            <p className="text-[10px] text-muted-foreground leading-tight">{opt.description}</p>
                                            {opt.recommended && (
                                                <Badge variant="secondary" className="mt-1 w-fit text-[10px] h-4 px-1 py-0 pointer-events-none">
                                                    Recommended
                                                </Badge>
                                            )}
                                        </div>
                                    ))}
                                </div>
                            </div>

                            {/* Timeouts */}
                            <div className="space-y-2">
                                <Label>Timeout Settings</Label>
                                <div className="bg-muted/30 p-3 rounded-md border border-border/50">
                                    {connectionType === "fixed" && (
                                        <div className="space-y-1.5 transition-opacity">
                                            <Label className="text-xs text-muted-foreground flex items-center gap-1">
                                                Device Timeout
                                            </Label>
                                            <div className="flex items-center gap-2">
                                                <Input
                                                    type="number"
                                                    value={deviceTimeout}
                                                    onChange={(e) => setDeviceTimeout(parseInt(e.target.value) || 0)}
                                                    className="w-full h-8"
                                                />
                                                <span className="text-xs text-muted-foreground shrink-0">sec</span>
                                            </div>
                                            <p className="text-[10px] text-muted-foreground/70 leading-tight">
                                                {suggestion.deviceTimeoutDesc}
                                            </p>
                                        </div>
                                    )}
                                    {connectionType === "removable" && (
                                        <div className="space-y-1.5 transition-opacity">
                                            <Label className="text-xs text-muted-foreground flex items-center gap-1">
                                                Idle Timeout
                                            </Label>
                                            <div className="flex items-center gap-2">
                                                <Input
                                                    type="number"
                                                    value={idleTimeout}
                                                    onChange={(e) => setIdleTimeout(parseInt(e.target.value) || 0)}
                                                    className="w-full h-8"
                                                />
                                                <span className="text-xs text-muted-foreground shrink-0">sec</span>
                                            </div>
                                            <p className="text-[10px] text-muted-foreground/70 leading-tight">
                                                {suggestion.idleTimeoutDesc}
                                            </p>
                                        </div>
                                    )}
                                </div>
                            </div>

                            {/* Mount Point */}
                            <div className="space-y-2">
                                <Label>Mount Point</Label>
                                <Input
                                    value={mountPoint}
                                    onChange={(e) => setMountPoint(e.target.value)}
                                    placeholder="/home/deck/Drives/..."
                                    className="h-9"
                                />
                                <div className="flex gap-2 items-center text-xs text-muted-foreground/80 px-1">
                                    <Info className="h-3 w-3 shrink-0" />
                                    <span>Default path allows mounting without root password.</span>
                                </div>
                            </div>

                            {/* Preview */}
                            <div className="space-y-2">
                                <Label>Fstab Entry Preview</Label>
                                <div className="rounded-md bg-muted p-2.5 overflow-x-auto border border-border shadow-inner">
                                    {preview ? (
                                        <code className="text-[11px] font-mono whitespace-pre block min-w-max">
                                            {preview.fstabLine}
                                        </code>
                                    ) : (
                                        <span className="text-xs text-muted-foreground">Generating preview...</span>
                                    )}
                                </div>
                            </div>
                        </div>
                    ) : (
                        <div className="flex items-center justify-center p-8 text-muted-foreground text-sm">
                            Loading recommendations...
                        </div>
                    )}
                </div>

                <DialogFooter className="flex-shrink-0 pt-4 mt-auto border-t border-border/10">
                    <Button variant="outline" onClick={() => onOpenChange(false)} className="h-9">Cancel</Button>
                    <Button onClick={() => handleMount(false)} disabled={loading || !suggestion} className="h-9">
                        {loading ? "Mounting..." : "Mount Device"}
                    </Button>
                </DialogFooter>
            </DialogContent>
        </Dialog>
    );
}
