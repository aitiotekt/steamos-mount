import { HardDrive, AlertTriangle, CheckCircle2, ChevronDown, ChevronUp, Gamepad2 } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { formatBytes } from "@/lib/utils";
import type { DeviceInfo } from "@/types";
import { useState } from "react";

interface DeviceCardProps {
    device: DeviceInfo;
    steamLibraries?: string[];
    onMount?: (device: DeviceInfo) => void;
    onUnmount?: (device: DeviceInfo) => void;
    onRepair?: (device: DeviceInfo) => void;
    onConfigureSteam?: (device: DeviceInfo) => void;
}

export function DeviceCard({
    device,
    steamLibraries,
    onMount,
    onUnmount,
    onRepair,
    onConfigureSteam,
}: DeviceCardProps) {
    const displayName = device.label || device.name;
    const fsLabel = device.fstype.toUpperCase();
    const [isOpen, setIsOpen] = useState(false);

    // Check for Steam library match
    const mountpoint = device.mountpoint;
    const steamLib = mountpoint ? steamLibraries?.find(lib => lib.startsWith(mountpoint)) : null;

    return (
        <Card className="hover:shadow-md transition-shadow flex flex-col h-full">
            <CardHeader className="pb-2">
                <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                        <HardDrive className="h-5 w-5 text-muted-foreground" />
                        <CardTitle className="text-lg">{displayName}</CardTitle>
                    </div>
                    <div className="flex gap-2">
                        <Badge variant="outline">{fsLabel}</Badge>
                        {device.isMounted ? (
                            <Badge variant="success">Mounted</Badge>
                        ) : device.isDirty ? (
                            <Badge variant="warning">Dirty</Badge>
                        ) : device.managedEntry ? (
                            <Badge variant="outline" className="border-yellow-500 text-yellow-600 hover:bg-yellow-50 dark:text-yellow-400 dark:hover:bg-yellow-950/20">Configured</Badge>
                        ) : (
                            <Badge variant="secondary">Not Mounted</Badge>
                        )}
                    </div>
                </div>
            </CardHeader>
            <CardContent className="flex flex-col flex-1">
                <div className="grid gap-2 text-sm items-center" style={{ gridTemplateColumns: '120px 1fr' }}>
                    <span className="text-muted-foreground">Device</span>
                    <span className="font-mono text-right">{device.path}</span>

                    <span className="text-muted-foreground">Size</span>
                    <span className="text-right">{formatBytes(device.size)}</span>

                    {device.mountpoint ? (
                        <>
                            <span className="text-muted-foreground">Mount Point</span>
                            <span className="font-mono text-xs text-right">{device.mountpoint}</span>
                        </>
                    ) : device.managedEntry ? (
                        <>
                            <span className="text-muted-foreground">Target Path</span>
                            <span className="font-mono text-xs text-right text-muted-foreground italic truncate" title={device.managedEntry.mountPoint}>
                                {device.managedEntry.mountPoint}
                            </span>
                        </>
                    ) : null}

                    {device.uuid && (
                        <>
                            <span className="text-muted-foreground">UUID</span>
                            <span className="font-mono text-xs break-all text-right">{device.uuid}</span>
                        </>
                    )}

                    {device.partuuid && (
                        <>
                            <span className="text-muted-foreground">PARTUUID</span>
                            <span className="font-mono text-xs break-all text-right">{device.partuuid}</span>
                        </>
                    )}

                    {/* Steam Library Display */}
                    {steamLib && (
                        <>
                            <span className="text-muted-foreground">Steam Library</span>
                            <span className="font-mono text-xs text-right truncate" title={steamLib}>
                                {steamLib}
                            </span>
                        </>
                    )}
                </div>

                {device.managedEntry?.rawContent && (
                    <Collapsible
                        open={isOpen}
                        onOpenChange={setIsOpen}
                        className="w-full mt-4"
                    >
                        <div className="flex items-center justify-between">
                            <span className="text-sm text-muted-foreground">Managed Fstab Entry</span>
                            <CollapsibleTrigger asChild>
                                <Button variant="ghost" size="sm" className="w-9 p-0">
                                    {isOpen ? (
                                        <ChevronUp className="h-4 w-4" />
                                    ) : (
                                        <ChevronDown className="h-4 w-4" />
                                    )}
                                    <span className="sr-only">Toggle</span>
                                </Button>
                            </CollapsibleTrigger>
                        </div>
                        <CollapsibleContent>
                            <div className="rounded-md bg-muted p-2">
                                <code className="text-xs font-mono break-all whitespace-pre-wrap">
                                    {device.managedEntry.rawContent}
                                </code>
                            </div>
                        </CollapsibleContent>
                    </Collapsible>
                )}

                <div className="flex flex-col gap-2 mt-auto pt-4">
                    {device.isMounted && (
                        <Button
                            variant="outline"
                            size="sm"
                            className="w-full"
                            onClick={() => onConfigureSteam?.(device)}
                        >
                            <Gamepad2 className="h-4 w-4 mr-1" />
                            Configure Steam Library
                        </Button>
                    )}

                    <div className="flex gap-2">
                        {device.isDirty && !device.isMounted && (
                            <Button
                                variant="outline"
                                size="sm"
                                className="flex-1"
                                onClick={() => onRepair?.(device)}
                            >
                                <AlertTriangle className="h-4 w-4 mr-1" />
                                Repair
                            </Button>
                        )}
                        {device.isMounted ? (
                            <Button
                                variant="outline"
                                size="sm"
                                className="flex-1"
                                onClick={() => onUnmount?.(device)}
                                disabled={!device.managedEntry}
                                title={!device.managedEntry ? "Only devices managed by this application can be unmounted" : "Unmount device"}
                            >
                                Unmount
                            </Button>
                        ) : (
                            <Button
                                size="sm"
                                className="flex-1"
                                onClick={() => onMount?.(device)}
                                disabled={device.isDirty}
                            >
                                <CheckCircle2 className="h-4 w-4 mr-1" />
                                Mount
                            </Button>
                        )}
                    </div>
                </div>
            </CardContent>
        </Card>
    );
}
