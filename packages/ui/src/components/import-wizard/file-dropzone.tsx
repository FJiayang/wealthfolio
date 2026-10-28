import { Button } from "../ui/button";
import { Icons } from "../ui/icons";
import { AnimatePresence, motion } from "motion/react";
import { useRef, useState } from "react";
import { useNumberFormatting } from "../formatting-provider";
import { Spinner } from "../ui/spinner";

interface FileDropzoneProps {
  file: File | null;
  labels: { remove: string; processing: string; choose: string; drop: string; hint: string };
  onFileChange: (file: File | null) => void;
  isLoading?: boolean;
  accept?: string;
  isValid?: boolean;
  error?: string | null;
}

export const FileDropzone = ({
  file,
  labels,
  onFileChange,
  isLoading = false,
  accept = "*",
  isValid = true,
  error = null,
}: FileDropzoneProps) => {
  const { formatDecimal } = useNumberFormatting();
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  };

  const handleDragLeave = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  };

  const handleDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);

    if (isLoading || file) return;
    if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
      onFileChange(e.dataTransfer.files[0]);
    }
  };

  const handleFileInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files.length > 0) {
      onFileChange(e.target.files[0]);
    }
  };

  const handleClick = () => {
    if (!file && !isLoading) {
      fileInputRef.current?.click();
    }
  };

  const handleRemoveFile = (e: React.MouseEvent) => {
    e.stopPropagation(); // Prevent triggering the parent div's click handler
    onFileChange(null);
    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  };

  // Determine the border and background colors based on state
  // Use same styling as AccountSelector - icons show status colors, not background
  const getBorderClasses = () => {
    if (isDragging) {
      return "border-primary bg-primary/5";
    }

    if (file && isValid && !error && !isLoading) {
      return "border-border bg-background";
    }

    return "border-border bg-background/50 hover:bg-background/80 hover:border-muted-foreground/50";
  };

  // Animation variants for icon containers
  const iconContainerVariants = {
    initial: {
      scale: 0.8,
      opacity: 0,
      rotate: -10,
    },
    animate: {
      scale: 1,
      opacity: 1,
      rotate: 0,
      transition: {
        type: "spring",
        stiffness: 260,
        damping: 20,
        duration: 0.5,
      },
    },
    exit: {
      scale: 0.8,
      opacity: 0,
      rotate: 10,
      transition: { duration: 0.3 },
    },
  } as const;

  // Animation variants for icons
  const iconVariants = {
    initial: { scale: 0.6, opacity: 0 },
    animate: {
      scale: 1,
      opacity: 1,
      transition: {
        delay: 0.1,
        type: "spring",
        stiffness: 300,
      },
    },
  } as const;

  // Use solid border when file is valid
  const useSolidBorder = file && isValid && !error && !isLoading;

  return (
    <div
      className={`group relative flex h-full flex-col justify-center rounded-lg border p-4 transition-colors ${useSolidBorder ? "" : "border-dashed"} ${getBorderClasses()} ${!file && !isLoading ? "cursor-pointer" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      role={!file ? "button" : undefined}
      tabIndex={!file && !isLoading ? 0 : undefined}
      aria-label={!file ? labels.choose : undefined}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          handleClick();
        }
      }}
      onClick={handleClick}
    >
      <input
        aria-label={labels.choose}
        type="file"
        ref={fileInputRef}
        onChange={handleFileInputChange}
        className="hidden"
        accept={accept}
        disabled={isLoading}
      />

      {file && !isLoading && (
        <div
          className="bg-background/90 dark:bg-background/95 pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg opacity-0 transition-opacity duration-200 group-focus-within:pointer-events-auto group-focus-within:opacity-90 group-hover:pointer-events-auto group-hover:opacity-90"
          onClick={(e) => e.stopPropagation()}
        >
          <div className="flex flex-col items-center gap-3">
            <Button
              size="sm"
              onClick={handleRemoveFile}
              disabled={isLoading}
              className="flex items-center gap-1.5 px-3"
            >
              <Icons.Trash className="h-4 w-4" />
              <span>{labels.remove}</span>
            </Button>
          </div>
        </div>
      )}

      <div className="flex flex-col items-center justify-center space-y-2">
        <AnimatePresence mode="wait">
          {isLoading ? (
            <motion.div
              key="loading"
              variants={iconContainerVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="bg-info-subtle flex h-10 w-10 items-center justify-center rounded-full shadow-sm"
            >
              <motion.div variants={iconVariants} initial="initial" animate="animate">
                <Spinner className="text-info h-5 w-5" />
              </motion.div>
            </motion.div>
          ) : file && (!isValid || error) ? (
            <motion.div
              key="error"
              variants={iconContainerVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="bg-destructive-subtle flex h-10 w-10 items-center justify-center rounded-full shadow-sm"
            >
              <motion.div variants={iconVariants} initial="initial" animate="animate">
                <Icons.AlertCircle className="text-destructive h-5 w-5" />
              </motion.div>
            </motion.div>
          ) : file ? (
            <motion.div
              key="file"
              variants={iconContainerVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="bg-success-subtle flex h-10 w-10 items-center justify-center rounded-full shadow-sm"
            >
              <motion.div variants={iconVariants} initial="initial" animate="animate">
                {isValid ? (
                  <Icons.CheckCircle className="text-success h-5 w-5" />
                ) : (
                  <Icons.FileText className="text-success h-5 w-5" />
                )}
              </motion.div>
            </motion.div>
          ) : (
            <motion.div
              key="upload"
              variants={iconContainerVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="bg-muted flex h-10 w-10 items-center justify-center rounded-full shadow-sm"
            >
              <motion.div variants={iconVariants} initial="initial" animate="animate">
                <Icons.Import className="text-muted-foreground h-5 w-5" />
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>

        <div className="space-y-0.5 text-center">
          <AnimatePresence mode="wait">
            {isLoading ? (
              <motion.p
                key="loading-text"
                initial={{ opacity: 0, y: 5 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -5 }}
                transition={{ duration: 0.2 }}
                className="text-xs font-medium"
              >
                {labels.processing}
              </motion.p>
            ) : file && (!isValid || error) ? (
              <motion.div
                key="error-info"
                initial={{ opacity: 0, y: 5 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -5 }}
                transition={{ duration: 0.2 }}
                className="space-y-0"
              >
                <p className="text-destructive font-medium">{file.name}</p>
                <p className="text-destructive text-xs">
                  {formatDecimal(file.size / 1024, { maximumFractionDigits: 2 })} KB
                </p>
              </motion.div>
            ) : file ? (
              <motion.div
                key="file-info"
                initial={{ opacity: 0, y: 5 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -5 }}
                transition={{ duration: 0.2 }}
                className="space-y-0"
              >
                <p className="text-xs font-medium">{file.name}</p>
                <p className="text-muted-foreground text-xs">
                  {formatDecimal(file.size / 1024, { maximumFractionDigits: 2 })} KB
                </p>
              </motion.div>
            ) : (
              <motion.div
                key="upload-text"
                initial={{ opacity: 0, y: 5 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -5 }}
                transition={{ duration: 0.2 }}
              >
                <p className="text-xs font-medium">
                  <span className="text-primary">{labels.choose}</span> {labels.drop}
                </p>
                <p className="text-muted-foreground text-xs">{labels.hint}</p>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
};
