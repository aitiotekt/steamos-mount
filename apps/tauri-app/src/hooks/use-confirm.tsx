import { createContext, useCallback, useContext, useState, type ReactNode } from 'react';
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from '@/components/ui/alert-dialog';

interface ConfirmOptions {
    title: string;
    description: string;
    variant?: "default" | "destructive";
    confirmText?: string;
    cancelText?: string;
}

interface ConfirmContextType {
    confirm: (options: ConfirmOptions) => Promise<boolean>;
}

const ConfirmContext = createContext<ConfirmContextType | undefined>(undefined);

export function ConfirmProvider({ children }: { children: ReactNode }) {
    const [open, setOpen] = useState(false);
    const [options, setOptions] = useState<ConfirmOptions>({
        title: "",
        description: "",
    });
    const [resolveRef, setResolveRef] = useState<((value: boolean) => void) | null>(
        null
    );

    const confirm = useCallback((options: ConfirmOptions) => {
        setOptions(options);
        setOpen(true);
        return new Promise<boolean>((resolve) => {
            setResolveRef(() => resolve);
        });
    }, []);

    const handleConfirm = () => {
        setOpen(false);
        resolveRef?.(true);
    };

    const handleCancel = () => {
        setOpen(false);
        resolveRef?.(false);
    };

    return (
        <ConfirmContext.Provider value={{ confirm }} >
            {children}
            < AlertDialog open={open} onOpenChange={setOpen} >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>{options.title} </AlertDialogTitle>
                        < AlertDialogDescription > {options.description} </AlertDialogDescription>
                    </AlertDialogHeader>
                    < AlertDialogFooter >
                        <AlertDialogCancel onClick={handleCancel}>
                            {options.cancelText || "Cancel"}
                        </AlertDialogCancel>
                        < AlertDialogAction
                            onClick={handleConfirm}
                            className={
                                options.variant === "destructive" ? "bg-destructive text-destructive-foreground hover:bg-destructive/90" : ""
                            }
                        >
                            {options.confirmText || "Confirm"}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </ConfirmContext.Provider >
    );
}

export function useConfirm() {
    const context = useContext(ConfirmContext);
    if (context === undefined) {
        throw new Error('useConfirm must be used within a ConfirmProvider');
    }
    return context;
}
