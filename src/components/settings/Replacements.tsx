import React, { useState, useRef } from "react";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Replacement, CapitalizationRule } from "@/bindings";
import { Trash2, ArrowRight, CaseUpper, CaseLower, Scissors, Pencil, GripVertical, Download, Upload, Regex } from "lucide-react";

const InfoTooltip: React.FC<{ text: string }> = ({ text }) => {
  const [showTooltip, setShowTooltip] = useState(false);
  const [tooltipPosition, setTooltipPosition] = useState<"top" | "bottom">("top");
  const tooltipTriggerRef = useRef<HTMLDivElement>(null);

  const handleTooltipEnter = () => {
    if (tooltipTriggerRef.current) {
      const rect = tooltipTriggerRef.current.getBoundingClientRect();
      const spaceAbove = rect.top;
      if (spaceAbove < 100) {
        setTooltipPosition("bottom");
      } else {
        setTooltipPosition("top");
      }
    }
    setShowTooltip(true);
  };

  return (
    <div 
      className="relative"
      ref={tooltipTriggerRef}
      onMouseEnter={handleTooltipEnter}
      onMouseLeave={() => setShowTooltip(false)}
    >
      <svg
        className="w-4 h-4 text-mid-gray cursor-help hover:text-logo-primary transition-colors duration-200 select-none"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        aria-label="More information"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
      {showTooltip && (
        <div className={`absolute ${tooltipPosition === "top" ? "bottom-full mb-2" : "top-full mt-2"} left-1/2 transform -translate-x-1/2 px-3 py-2 bg-background border border-mid-gray/80 rounded-lg shadow-lg z-50 w-64 whitespace-normal animate-in fade-in-0 zoom-in-95 duration-200`}>
          <p className="text-sm text-center leading-relaxed">
            {text}
          </p>
          <div className={`absolute ${tooltipPosition === "top" ? "top-full border-t-mid-gray/80 border-t-[6px]" : "bottom-full border-b-mid-gray/80 border-b-[6px]"} left-1/2 transform -translate-x-1/2 w-0 h-0 border-l-[6px] border-r-[6px] border-l-transparent border-r-transparent`}></div>
        </div>
      )}
    </div>
  );
};

const getScrollParent = (node: HTMLElement | null): HTMLElement | null => {
  if (!node) return null;
  const style = window.getComputedStyle(node);
  const overflowY = style.overflowY;
  const isScrollable = overflowY !== 'visible' && overflowY !== 'hidden';
  
  if (isScrollable && node.scrollHeight > node.clientHeight) {
    return node;
  }
  return getScrollParent(node.parentElement);
};

export const Replacements: React.FC = () => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [search, setSearch] = useState("");
  const [replace, setReplace] = useState("");
  const [isRegex, setIsRegex] = useState(false);
  const [removePunctuation, setRemovePunctuation] = useState(false);
  const [capitalization, setCapitalization] = useState<CapitalizationRule>("none");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [filterText, setFilterText] = useState("");
  const [lastImportedRange, setLastImportedRange] = useState<{start: number, count: number} | null>(null);
  
  // Drag and drop state
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dropIndexRef = useRef<number | null>(null);
  const scrollInterval = useRef<number | null>(null);
  const scrollSpeed = useRef<number>(0);
  const formRef = useRef<HTMLDivElement>(null);
  
  const replacements = getSetting("replacements") || [];

  const renderText = (text: string) => {
    if (!text) return <span className="opacity-50 italic">empty</span>;
    return text.split('').map((char, i) => 
      char === ' ' ? <span key={i} className="opacity-30">·</span> : char
    );
  };

  React.useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && editingIndex !== null) {
        resetForm();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [editingIndex]);

  const searchCounts = replacements.reduce((acc, item) => {
    const key = item.search.trim().toLowerCase();
    acc[key] = (acc[key] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  const handleAddOrUpdate = () => {
    if (search && replace) {
      const newReplacement: Replacement = { 
        search, 
        replace,
        is_regex: isRegex,
        remove_surrounding_punctuation: removePunctuation,
        capitalization_rule: capitalization
      };

      let newReplacements = [...replacements];
      if (editingIndex !== null) {
        newReplacements[editingIndex] = newReplacement;
      } else {
        newReplacements = [...replacements, newReplacement];
      }
      
      updateSetting("replacements", newReplacements);
      setLastImportedRange(null);
      resetForm();
    }
  };

  const resetForm = () => {
    setSearch("");
    setReplace("");
    setIsRegex(false);
    setRemovePunctuation(false);
    setCapitalization("none");
    setEditingIndex(null);
  };

  const handleEdit = (index: number) => {
    const item = replacements[index];
    setSearch(item.search);
    setReplace(item.replace);
    setIsRegex(item.is_regex || false);
    setRemovePunctuation(item.remove_surrounding_punctuation || false);
    setCapitalization(item.capitalization_rule || "none");
    setEditingIndex(index);
    formRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  const handleRemove = (index: number) => {
    const newReplacements = [...replacements];
    newReplacements.splice(index, 1);
    updateSetting("replacements", newReplacements);
    
    // Adjust range if needed, or just clear it to be safe/simple
    if (lastImportedRange) {
        if (index < lastImportedRange.start) {
            setLastImportedRange({ ...lastImportedRange, start: lastImportedRange.start - 1 });
        } else if (index >= lastImportedRange.start && index < lastImportedRange.start + lastImportedRange.count) {
            setLastImportedRange({ ...lastImportedRange, count: lastImportedRange.count - 1 });
        }
    }

    if (editingIndex === index) {
      resetForm();
    }
  };

  const handleExport = async () => {
    const dataStr = JSON.stringify(replacements, null, 2);
    
    try {
      // Try to use the File System Access API if available (modern browsers/webviews)
      // @ts-ignore - showSaveFilePicker is not yet in all TS definitions
      if (window.showSaveFilePicker) {
        // @ts-ignore
        const handle = await window.showSaveFilePicker({
          suggestedName: 'handy-replacements.json',
          types: [{
            description: 'JSON Files',
            accept: {'application/json': ['.json']},
          }],
        });
        const writable = await handle.createWritable();
        await writable.write(dataStr);
        await writable.close();
        return;
      }
    } catch (err) {
      // User cancelled or API failed, fall back to download
      console.log("File System Access API failed or cancelled, falling back to download", err);
    }

    // Fallback for older browsers or if user cancelled the picker (though usually we stop there)
    // But if the API isn't supported, we do this:
    const blob = new Blob([dataStr], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "handy-replacements.json";
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  };

  const handleImport = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        const content = event.target?.result as string;
        const imported = JSON.parse(content);
        
        if (Array.isArray(imported)) {
            // Basic validation: check if items look like replacements
            const isValid = imported.every(item => 
                typeof item === 'object' && 
                item !== null && 
                'search' in item && 
                'replace' in item
            );

            if (isValid) {
                // Append imported items to existing replacements, allowing duplicates
                const start = replacements.length;
                const count = imported.length;
                const newReplacements = [...replacements, ...imported];
                updateSetting("replacements", newReplacements);
                setLastImportedRange({ start, count });
            } else {
                console.error("Invalid format: items must have search and replace fields");
            }
        } else {
            console.error("Invalid format: expected an array");
        }
      } catch (error) {
        console.error("Failed to parse JSON", error);
      }
    };
    reader.readAsText(file);
    e.target.value = ""; 
  };

  const handleDragStart = (e: React.MouseEvent, index: number) => {
    e.preventDefault();
    setDraggingIndex(index);
    
    const scrollContainer = getScrollParent(listRef.current);
    
    // Start scroll loop
    scrollInterval.current = window.setInterval(() => {
      if (scrollSpeed.current !== 0 && scrollContainer) {
        scrollContainer.scrollBy(0, scrollSpeed.current);
      }
    }, 16);

    // Define handlers first so they can reference each other if needed (via cleanup)
    let handleMouseMove: (e: MouseEvent) => void;
    let handleMouseUp: () => void;
    let handleKeyDown: (e: KeyboardEvent) => void;

    const cleanup = () => {
      if (scrollInterval.current) {
        clearInterval(scrollInterval.current);
        scrollInterval.current = null;
      }
      scrollSpeed.current = 0;

      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      document.removeEventListener('keydown', handleKeyDown);
    };
    
    handleMouseMove = (moveEvent: MouseEvent) => {
      if (!listRef.current) return;
      
      // Handle scrolling
      const SCROLL_ZONE = 100;
      const MAX_SPEED = 20;
      
      let topZone = SCROLL_ZONE;
      let bottomZone = window.innerHeight - SCROLL_ZONE;
      
      if (scrollContainer) {
        const rect = scrollContainer.getBoundingClientRect();
        topZone = rect.top + SCROLL_ZONE;
        bottomZone = rect.bottom - SCROLL_ZONE;
      }
      
      if (moveEvent.clientY < topZone) {
        const intensity = (topZone - moveEvent.clientY) / SCROLL_ZONE;
        scrollSpeed.current = -Math.max(2, Math.round(MAX_SPEED * intensity));
      } else if (moveEvent.clientY > bottomZone) {
        const intensity = (moveEvent.clientY - bottomZone) / SCROLL_ZONE;
        scrollSpeed.current = Math.max(2, Math.round(MAX_SPEED * intensity));
      } else {
        scrollSpeed.current = 0;
      }
      
      const items = Array.from(listRef.current.children).filter(child => !child.classList.contains('drag-indicator')) as HTMLElement[];
      let newDropIndex = items.length;
      
      for (let i = 0; i < items.length; i++) {
        const rect = items[i].getBoundingClientRect();
        const middleY = rect.top + rect.height / 2;
        
        if (moveEvent.clientY < middleY) {
          newDropIndex = i;
          break;
        }
      }
      
      if (newDropIndex !== dropIndexRef.current) {
          dropIndexRef.current = newDropIndex;
          setDropIndex(newDropIndex);
      }
    };

    handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        cleanup();
        setDraggingIndex(null);
        setDropIndex(null);
        dropIndexRef.current = null;
      }
    };

    handleMouseUp = () => {
      cleanup();
      
      const finalDropIndex = dropIndexRef.current;
      
      if (finalDropIndex !== null && finalDropIndex !== index && finalDropIndex !== index + 1) {
        const newReplacements = [...replacements];
        const [movedItem] = newReplacements.splice(index, 1);
        
        let targetIndex = finalDropIndex;
        if (targetIndex > index) {
          targetIndex -= 1;
        }
        
        newReplacements.splice(targetIndex, 0, movedItem);
        updateSetting("replacements", newReplacements);
        
        // Adjust editing index if needed
        if (editingIndex === index) {
            setEditingIndex(targetIndex);
        } else if (editingIndex !== null) {
            // If we moved an item from before editingIndex to after, decrement editingIndex
            if (index < editingIndex && targetIndex >= editingIndex) {
                setEditingIndex(editingIndex - 1);
            }
            // If we moved an item from after editingIndex to before, increment editingIndex
            else if (index > editingIndex && targetIndex <= editingIndex) {
                setEditingIndex(editingIndex + 1);
            }
        }
      }
      
      setDraggingIndex(null);
      setDropIndex(null);
      dropIndexRef.current = null;
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    document.addEventListener('keydown', handleKeyDown);
  };

  return (
    <div className="flex flex-col gap-4 w-full">
      <SettingsGroup title="Text Replacements">
        <div className="flex flex-col gap-3 w-full p-3" ref={formRef}>
          <div className="flex items-center gap-2 w-full">
            <Input
              type="text"
              className="flex-1"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Word to replace"
              variant="compact"
            />
            <ArrowRight className="text-mid-gray w-4 h-4" />
            <Input
              type="text"
              className="flex-1"
              value={replace}
              onChange={(e) => setReplace(e.target.value)}
              placeholder="Replacement"
              variant="compact"
            />
          </div>
          
          <div className="flex items-center gap-4 text-sm text-mid-gray">
            <div className="flex items-center gap-2">
              <label className="flex items-center gap-2 cursor-pointer select-none hover:text-white transition-colors">
                <Regex className="w-4 h-4" />
                <span>Regex</span>
              </label>
              <InfoTooltip text="Use regular expressions for advanced matching (e.g. 'hello|hi' matches both)" />
              <input
                  type="checkbox"
                  checked={isRegex}
                  onChange={(e) => setIsRegex(e.target.checked)}
                  className="rounded border-mid-gray bg-transparent text-logo-primary focus:ring-logo-primary"
                />
            </div>

            <div className="h-4 w-px bg-mid-gray/30" />

            <div className="flex items-center gap-2">
              <label className="flex items-center gap-2 cursor-pointer select-none hover:text-white transition-colors">

                <Scissors className="w-4 h-4" />
                <span>Trim punctuation</span>                
              </label>
              <InfoTooltip text='Removes punctuation and spaces around the word (e.g. ". word ," → "word")' />
              <input
                  type="checkbox"
                  checked={removePunctuation}
                  onChange={(e) => setRemovePunctuation(e.target.checked)}
                  className="rounded border-mid-gray bg-transparent text-logo-primary focus:ring-logo-primary"
                />
            </div>

            <div className="h-4 w-px bg-mid-gray/30" />

            <div className="flex items-center gap-2">
              <span>Next word:</span>
              <InfoTooltip text="Controls capitalization of the word immediately following the replacement. 'None' preserves the original casing." />
              <div className="flex bg-mid-gray/10 rounded-md p-0.5">
                <button
                  onClick={() => setCapitalization("none")}
                  className={`px-2 py-1 rounded text-xs transition-colors ${
                    capitalization === "none" 
                      ? "bg-mid-gray/30 text-white" 
                      : "hover:bg-mid-gray/20"
                  }`}
                >
                  None
                </button>
                <button
                  onClick={() => setCapitalization("force_uppercase")}
                  className={`px-2 py-1 rounded text-xs transition-colors flex items-center gap-1 ${
                    capitalization === "force_uppercase" 
                      ? "bg-mid-gray/30 text-white" 
                      : "hover:bg-mid-gray/20"
                  }`}
                  title="Force Uppercase"
                >
                  <CaseUpper className="w-3 h-3" />
                </button>
                <button
                  onClick={() => setCapitalization("force_lowercase")}
                  className={`px-2 py-1 rounded text-xs transition-colors flex items-center gap-1 ${
                    capitalization === "force_lowercase" 
                      ? "bg-mid-gray/30 text-white" 
                      : "hover:bg-mid-gray/20"
                  }`}
                  title="Force Lowercase"
                >
                  <CaseLower className="w-3 h-3" />
                </button>
              </div>
            </div>
          </div>

          <div className="flex gap-2">
            <Button
              onClick={handleAddOrUpdate}
              disabled={!search || !replace || isUpdating("replacements")}
              variant="primary"
              size="md"
              className="flex-1"
            >
              {editingIndex !== null ? "Update Replacement" : "Add Replacement"}
            </Button>
            {editingIndex !== null && (
              <Button
                onClick={resetForm}
                variant="ghost"
                size="md"
                className="text-mid-gray hover:text-white"
              >
                Cancel
              </Button>
            )}
          </div>
        </div>
      </SettingsGroup>

      {replacements.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="px-1">
            <Input
              type="text"
              value={filterText}
              onChange={(e) => setFilterText(e.target.value)}
              placeholder="Filter replacements..."
              variant="compact"
              className="w-full"
            />
          </div>
          <div 
            ref={listRef}
            className="flex flex-col gap-2"
          >
          {replacements.map((item, index) => {
            const isDuplicate = searchCounts[item.search.trim().toLowerCase()] > 1;
            const isNewImport = lastImportedRange && index >= lastImportedRange.start && index < (lastImportedRange.start + lastImportedRange.count);
            
            const matchesFilter = !filterText || 
              item.search.toLowerCase().includes(filterText.toLowerCase()) || 
              item.replace.toLowerCase().includes(filterText.toLowerCase());

            if (!matchesFilter) return null;

            return (
            <React.Fragment key={index}>
              {dropIndex === index && (draggingIndex === null || (dropIndex !== draggingIndex && dropIndex !== draggingIndex + 1)) && !filterText && (
                <div className="h-0.5 bg-logo-primary w-full rounded-full animate-pulse drag-indicator" />
              )}
              <div 
                className={`flex items-center gap-3 p-2 bg-background border border-mid-gray/20 rounded-lg group transition-all ${
                  draggingIndex === index ? 'opacity-50 scale-95 border-dashed border-mid-gray' : 'hover:border-mid-gray/40'
                } ${isDuplicate ? '!border-orange-500/50 bg-orange-500/5' : ''} ${isNewImport ? '!border-green-500/50 bg-green-500/5' : ''}`}
              >
                <div 
                  className={`text-mid-gray p-1 ${filterText ? 'opacity-30 cursor-not-allowed' : 'cursor-grab active:cursor-grabbing hover:text-white'}`}
                  onMouseDown={(e) => !filterText && handleDragStart(e, index)}
                >
                  <GripVertical size={16} />
                </div>
                <div className="flex-1 flex flex-col gap-1 min-w-0">
                  <div className="flex items-center gap-2 text-sm">
                    <span className="font-mono text-xs bg-mid-gray/20 rounded px-1 py-0.5 text-white whitespace-pre border border-mid-gray/30 inline-block max-w-[12rem] overflow-hidden text-ellipsis align-middle" title={item.search}>
                        {renderText(item.search)}
                    </span>
                    <ArrowRight className="text-mid-gray w-3 h-3 flex-shrink-0" />
                    <span className="font-mono text-xs bg-logo-primary/10 rounded px-1 py-0.5 text-logo-primary whitespace-pre border border-logo-primary/20 inline-block max-w-[12rem] overflow-hidden text-ellipsis align-middle" title={item.replace}>
                        {renderText(item.replace)}
                    </span>
                    <div className="ml-auto flex items-center gap-2">
                        {isDuplicate && (
                        <span className="text-[10px] uppercase tracking-wider font-bold text-orange-400 bg-orange-400/10 px-1.5 py-0.5 rounded border border-orange-400/20">
                            Duplicate
                        </span>
                        )}
                        {isNewImport && <span title="Newly imported">✨</span>}
                    </div>
                  </div>
                  <div className="flex items-center gap-3 text-xs text-mid-gray">
                    {item.is_regex && (
                      <span className="flex items-center gap-1 text-logo-primary" title="Regular Expression">
                        <Regex className="w-3 h-3" /> Regex
                      </span>
                    )}
                    {item.remove_surrounding_punctuation && (
                      <span className="flex items-center gap-1" title="Trims surrounding punctuation">
                        <Scissors className="w-3 h-3" /> Trim
                      </span>
                    )}
                    {item.capitalization_rule !== "none" && (
                      <span className="flex items-center gap-1">
                        {item.capitalization_rule === "force_uppercase" ? (
                          <><CaseUpper className="w-3 h-3" /> Upper</>
                        ) : (
                          <><CaseLower className="w-3 h-3" /> Lower</>
                        )}
                      </span>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-all">
                  <Button
                    onClick={() => handleEdit(index)}
                    variant="ghost"
                    size="sm"
                    className="text-mid-gray hover:text-white hover:bg-mid-gray/20"
                  >
                    <Pencil size={16} />
                  </Button>
                  <Button
                    onClick={() => handleRemove(index)}
                    variant="ghost"
                    size="sm"
                    className="text-mid-gray hover:text-red-400 hover:bg-red-400/10"
                  >
                    <Trash2 size={16} />
                  </Button>
                </div>
              </div>
            </React.Fragment>
          )})}
          {dropIndex === replacements.length && (draggingIndex === null || (dropIndex !== draggingIndex && dropIndex !== draggingIndex + 1)) && !filterText && (
            <div className="h-0.5 bg-logo-primary w-full rounded-full animate-pulse drag-indicator" />
          )}
        </div>
        </div>
      )}

      <div className="flex gap-2 mt-2 pt-2 border-t border-mid-gray/20">
        <Button variant="secondary" size="sm" onClick={handleExport} className="flex items-center gap-2">
            <Download size={14} /> Export
        </Button>
        <div>
            <input
                type="file"
                accept=".json"
                ref={fileInputRef}
                onChange={handleImport}
                className="hidden"
            />
            <Button 
                variant="secondary" 
                size="sm" 
                className="flex items-center gap-2"
                onClick={() => fileInputRef.current?.click()}
            >
                <Upload size={14} /> Import
            </Button>
        </div>
      </div>
    </div>
  );
};
