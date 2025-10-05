import { HardDrive, AlertTriangle, CheckCircle2, ChevronDown, ChevronUp, Gamepad2, X, CloudOff } from "lucide-react";
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
    onDeconfigure?: (device: DeviceInfo) => void;
    onRepair?: (device: DeviceInfo) => void;
    onConfigureSteam?: (device: DeviceInfo) => void;
}

export function DeviceCard({
    device,
    steamLibraries,
    onMount,
    onUnmount,
    onDeconfigure,
    onRepair,
    onConfigureSteam,
}: DeviceCardProps) {
    const displayName = device.label || device.name;
    const fsLabel = device.fstype.toUpperCase();
    const [isOpen, setIsOpen] = useState(false);

    // Check for Steam library match
    const mountpoint = device.mountpoint || device.managedEntry?.mountPoint;
    const steamLib = mountpoint ? steamLibraries?.find(lib => lib.startsWith(mountpoint)) : null;

    // Status badge logic
    const renderStatusBadge = () => {
        if (device.isOffline) {
            return (
                <Badge variant="outline" className="border-gray-500 text-gray-600 dark:text-gray-400">
                    <CloudOff className="h-3 w-3 mr-1" />
                    Offline
                </Badge>
            );
        }
        if (device.isMounted) {
            return <Badge variant="success">Mounted</Badge>;
        }
        if (device.isDirty) {
            return <Badge variant="warning">Dirty</Badge>;
        }
        if (device.managedEntry) {
            return (
                <Badge variant="outline" className="border-yellow-500 text-yellow-600 hover:bg-yellow-50 dark:text-yellow-400 dark:hover:bg-yellow-950/20">
                    Configured
                </Badge>
            );
        }
        return <Badge variant="secondary">Not Mounted</Badge>;
    };

    return (
        <Card className={`hover:shadow-md transition-shadow flex flex-col h-full ${device.isOffline ? 'opacity-75' : ''}`}>
            <CardHeader className="pb-2">
                <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                        <HardDrive className={`h-5 w-5 ${device.isOffline ? 'text-gray-400' : 'text-muted-foreground'}`} />
                        <CardTitle className="text-lg">{displayName}</CardTitle>
                    </div>
                    <div className="flex gap-1.5 flex-wrap justify-end items-center content-start max-w-[60%]">
                        {device.transport && (
                            <Badge variant="outline" className="px-1.5 h-5 text-[10px] font-mono uppercase bg-background">
                                {device.transport}
                            </Badge>
                        )}
                        {device.rota !== undefined && (
                            <Badge variant="secondary" className="px-1.5 h-5 text-[10px]">
                                {device.rota ? "ROTA" : "FLASH"}
                            </Badge>
                        )}
                        {device.removable && (
                            <Badge variant="outline" className="px-1.5 h-5 text-[10px] bg-background">
                                Removable
                            </Badge>
                        )}
                        <Badge variant="outline" className="px-1.5 h-5 text-[10px] font-mono bg-background">{fsLabel}</Badge>
                        {renderStatusBadge()}
                    </div>
                </div>
            </CardHeader>
            <CardContent className="flex flex-col flex-1">
                <div className="grid gap-2 text-sm items-center" style={{ gridTemplateColumns: '120px 1fr' }}>
                    {/* Device path - only show for online devices */}
                    {!device.isOffline && device.path && (
                        <>
                            <span className="text-muted-foreground">Device</span>
                            <span className="font-mono text-right">{device.path}</span>
                        </>
                    )}

                    {/* Size - only show for online devices with valid size */}
                    {!device.isOffline && device.size > 0 && (
                        <>
                            <span className="text-muted-foreground">Size</span>
                            <span className="text-right">{formatBytes(device.size)}</span>
                        </>
                    )}

                    {
                        device.mountpoint && (
                            <>
                                <span className="text-muted-foreground">Mount Point</span>
                                <span className="font-mono text-xs text-right">{device.mountpoint}</span>
                            </>
                        )
                    }

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
                    {/* Configure Steam Library - show for mounted devices OR offline devices (to open settings) */}
                    {(device.isMounted || device.isOffline) && (
                        <Button
                            variant="outline"
                            size="sm"
                            className="w-full"
                            onClick={() => onConfigureSteam?.(device)}
                        >
                            <Gamepad2 className="h-4 w-4 mr-1" />
                            {device.isOffline ? "Open Steam Storage" : "Configure Steam Library"}
                        </Button>
                    )}

                    {/* Deconfigure - show for managed devices that are not mounted */}
                    {device.managedEntry && !device.isMounted && (
                        <Button
                            variant="outline"
                            size="sm"
                            className="w-full"
                            onClick={() => onDeconfigure?.(device)}
                        >
                            <X className="h-4 w-4 mr-1" />
                            Deconfigure Fstab Entry
                        </Button>
                    )}

                    {/* Mount/Unmount/Repair - only for online devices */}
                    {!device.isOffline && (
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
                    )}
                </div>
            </CardContent>
        </Card>
    );
}

