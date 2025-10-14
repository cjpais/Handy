import React, { useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { RegexFilter } from "../../lib/types";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface RegexFiltersProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

interface FilterFormData {
  name: string;
  pattern: string;
  replacement: string;
}

export const RegexFilters: React.FC<RegexFiltersProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const {
      getRegexFilters,
      addRegexFilter,
      updateRegexFilter,
      deleteRegexFilter,
      toggleRegexFilter,
    } = useSettingsStore();

    const [filters, setFilters] = useState<RegexFilter[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [showAddForm, setShowAddForm] = useState(false);
    const [formData, setFormData] = useState<FilterFormData>({
      name: "",
      pattern: "",
      replacement: "",
    });
    const [formErrors, setFormErrors] = useState<Partial<FilterFormData>>({});

    // Load filters on component mount
    useEffect(() => {
      loadFilters();
    }, []);

    const loadFilters = async () => {
      try {
        setIsLoading(true);
        const loadedFilters = await getRegexFilters();
        setFilters(loadedFilters);
      } catch (error) {
        console.error("Failed to load regex filters:", error);
      } finally {
        setIsLoading(false);
      }
    };

    const validateForm = (data: FilterFormData): Partial<FilterFormData> => {
      const errors: Partial<FilterFormData> = {};

      if (!data.name.trim()) {
        errors.name = "Name is required";
      }

      if (!data.pattern.trim()) {
        errors.pattern = "Pattern is required";
      } else {
        try {
          new RegExp(data.pattern);
        } catch (e) {
          errors.pattern = "Invalid regex pattern";
        }
      }

      if (!data.replacement.trim()) {
        errors.replacement = "Replacement is required";
      }

      return errors;
    };

    const handleSubmit = async (e: React.FormEvent) => {
      e.preventDefault();
      
      const errors = validateForm(formData);
      setFormErrors(errors);

      if (Object.keys(errors).length > 0) {
        return;
      }

      try {
        if (editingId) {
          // Update existing filter
          const existingFilter = filters.find(f => f.id === editingId);
          if (existingFilter) {
            await updateRegexFilter(
              editingId,
              formData.name,
              formData.pattern,
              formData.replacement,
              existingFilter.enabled
            );
          }
          setEditingId(null);
        } else {
          // Add new filter
          await addRegexFilter(formData.name, formData.pattern, formData.replacement);
          setShowAddForm(false);
        }

        // Reset form and reload filters
        setFormData({ name: "", pattern: "", replacement: "" });
        setFormErrors({});
        await loadFilters();
      } catch (error) {
        console.error("Failed to save regex filter:", error);
        // You might want to show an error message to the user here
      }
    };

    const handleEdit = (filter: RegexFilter) => {
      setFormData({
        name: filter.name,
        pattern: filter.pattern,
        replacement: filter.replacement,
      });
      setEditingId(filter.id);
      setShowAddForm(true);
    };

    const handleDelete = async (id: string) => {
      try {
        await deleteRegexFilter(id);
        await loadFilters();
      } catch (error) {
        console.error("Failed to delete regex filter:", error);
      }
    };

    const handleToggle = async (id: string, enabled: boolean) => {
      try {
        await toggleRegexFilter(id, enabled);
        await loadFilters();
      } catch (error) {
        console.error("Failed to toggle regex filter:", error);
      }
    };

    const handleCancel = () => {
      setShowAddForm(false);
      setEditingId(null);
      setFormData({ name: "", pattern: "", replacement: "" });
      setFormErrors({});
    };

    if (isLoading) {
      return (
        <SettingContainer
          title="Regex Filters"
          description="Apply regular expression filters to modify transcription output text."
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="text-sm text-gray-500">Loading filters...</div>
        </SettingContainer>
      );
    }

    return (
      <>
        <SettingContainer
          title="Regex Filters"
          description="Apply regular expression filters to modify transcription output text. Filters are applied in order after custom word correction."
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="space-y-4">
            {/* Add/Edit Form */}
            {showAddForm && (
              <form onSubmit={handleSubmit} className="space-y-3 p-4 border border-mid-gray/20 rounded-lg">
                <div>
                  <Input
                    type="text"
                    value={formData.name}
                    onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                    placeholder="Filter name"
                    variant="compact"
                    className={formErrors.name ? "border-red-500" : ""}
                  />
                  {formErrors.name && (
                    <div className="text-red-500 text-xs mt-1">{formErrors.name}</div>
                  )}
                </div>

                <div>
                  <Input
                    type="text"
                    value={formData.pattern}
                    onChange={(e) => setFormData({ ...formData, pattern: e.target.value })}
                    placeholder="Regex pattern (e.g., \\b(um|uh)\\b)"
                    variant="compact"
                    className={formErrors.pattern ? "border-red-500" : ""}
                  />
                  {formErrors.pattern && (
                    <div className="text-red-500 text-xs mt-1">{formErrors.pattern}</div>
                  )}
                </div>

                <div>
                  <Input
                    type="text"
                    value={formData.replacement}
                    onChange={(e) => setFormData({ ...formData, replacement: e.target.value })}
                    placeholder="Replacement text (use empty string to remove)"
                    variant="compact"
                    className={formErrors.replacement ? "border-red-500" : ""}
                  />
                  {formErrors.replacement && (
                    <div className="text-red-500 text-xs mt-1">{formErrors.replacement}</div>
                  )}
                </div>

                <div className="flex gap-2">
                  <Button type="submit" variant="primary" size="sm">
                    {editingId ? "Update" : "Add"} Filter
                  </Button>
                  <Button type="button" onClick={handleCancel} variant="secondary" size="sm">
                    Cancel
                  </Button>
                </div>
              </form>
            )}

            {/* Add Button */}
            {!showAddForm && (
              <Button
                onClick={() => setShowAddForm(true)}
                variant="primary"
                size="md"
              >
                Add Regex Filter
              </Button>
            )}
          </div>
        </SettingContainer>

        {/* Filters List */}
        {filters.length > 0 && (
          <div className={`space-y-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20 p-4"}`}>
            {filters.map((filter) => (
              <div
                key={filter.id}
                className="flex items-center justify-between p-3 border border-mid-gray/10 rounded-lg"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={filter.enabled}
                      onChange={(e) => handleToggle(filter.id, e.target.checked)}
                      className="rounded"
                    />
                    <div className="font-medium text-sm">{filter.name}</div>
                  </div>
                  <div className="text-xs text-gray-500 mt-1 font-mono">
                    /{filter.pattern}/ → "{filter.replacement}"
                  </div>
                </div>
                <div className="flex gap-1 ml-2">
                  <Button
                    onClick={() => handleEdit(filter)}
                    variant="secondary"
                    size="sm"
                    className="px-2"
                  >
                    Edit
                  </Button>
                  <Button
                    onClick={() => handleDelete(filter.id)}
                    variant="secondary"
                    size="sm"
                    className="px-2 text-red-600 hover:text-red-700"
                  >
                    Delete
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </>
    );
  }
);

RegexFilters.displayName = "RegexFilters";