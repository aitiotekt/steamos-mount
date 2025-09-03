import { atom } from 'jotai';

export interface AlertDialogState {
    open: boolean;
    title: string;
    description: string;
    variant?: 'default' | 'destructive';
    onConfirm: () => void;
    onCancel?: () => void;
}

export const alertDialogAtom = atom<AlertDialogState>({
    open: false,
    title: '',
    description: '',
    onConfirm: () => { },
});
