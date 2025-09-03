import type { ReactNode } from 'react';
import { Toaster } from '@/components/ui/sonner';
import { GlobalAlertDialog } from '@/components/global-alert-dialog';
import { Provider as JotaiProvider } from 'jotai';
import { ConfirmProvider } from '@/hooks/use-confirm';

interface ProvidersProps {
    children: ReactNode;
}

export function Providers({ children }: ProvidersProps) {
    return (
        <JotaiProvider>
            {/* ThemeProvider implementation is optional but good practice if needed later */}
            {/* For now we just wrap children, assuming ThemeProvider exists or we skip it if not */}
            <ConfirmProvider>
                {children}
            </ConfirmProvider>
            <Toaster />
            <GlobalAlertDialog />
        </JotaiProvider>
    );
}
