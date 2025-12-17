import React, { useEffect, useCallback } from "react";

interface ModalProps {
    isOpen: boolean;
    onClose: () => void;
    title: string;
    children: React.ReactNode;
}

export const Modal: React.FC<ModalProps> = ({
    isOpen,
    onClose,
    title,
    children,
}) => {
    const handleKeyDown = useCallback(
        (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                onClose();
            }
        },
        [onClose]
    );

    useEffect(() => {
        if (isOpen) {
            document.addEventListener("keydown", handleKeyDown);
            document.body.style.overflow = "hidden";
            return () => {
                document.removeEventListener("keydown", handleKeyDown);
                document.body.style.overflow = "";
            };
        }
    }, [isOpen, handleKeyDown]);

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 z-[200] flex items-center justify-center">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/60 backdrop-blur-sm animate-in fade-in-0 duration-200"
                onClick={onClose}
            />

            {/* Modal Container */}
            <div className="relative z-10 w-full max-w-md mx-4 bg-background border border-mid-gray/40 rounded-xl shadow-2xl animate-in fade-in-0 zoom-in-95 slide-in-from-bottom-4 duration-300">
                {/* Header */}
                <div className="flex items-center justify-between px-5 py-4 border-b border-mid-gray/20">
                    <h2 className="text-lg font-semibold">{title}</h2>
                    <button
                        onClick={onClose}
                        className="p-1 rounded-lg hover:bg-mid-gray/20 transition-colors duration-150 cursor-pointer"
                        aria-label="Close modal"
                    >
                        <svg
                            className="w-5 h-5 text-mid-gray hover:text-text"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                        >
                            <path
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth={2}
                                d="M6 18L18 6M6 6l12 12"
                            />
                        </svg>
                    </button>
                </div>

                {/* Content */}
                <div className="px-5 py-4">{children}</div>
            </div>
        </div>
    );
};
