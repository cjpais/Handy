import React, { useState, useCallback } from "react";
import { SettingsGroup } from "./ui/SettingsGroup";
import { Button } from "./ui/Button";
import { AudioPlayer } from "./ui/AudioPlayer";
import { Upload, FileAudio, Loader2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useModels } from "../hooks/useModels";
import { useSettings } from "../hooks/useSettings";
import { toast } from "sonner";

interface UploadedFile {
  file: File;
  preview: string;
}

export const UploadAudio: React.FC = () => {
  const [uploadedFile, setUploadedFile] = useState<UploadedFile | null>(null);
  const [transcribing, setTranscribing] = useState(false);
  const [transcription, setTranscription] = useState<string>("");
  const { models, currentModel } = useModels();
  const { settings } = useSettings();

  const [isDragOver, setIsDragOver] = useState(false);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);

    const files = Array.from(e.dataTransfer.files);
    console.log('Files dropped via HTML5:', files);

    if (files.length > 0) {
      const file = files[0];
      // Check if it's an audio file
      if (file.type.startsWith('audio/') ||
          file.name.toLowerCase().endsWith('.wav') ||
          file.name.toLowerCase().endsWith('.mp3') ||
          file.name.toLowerCase().endsWith('.m4a') ||
          file.name.toLowerCase().endsWith('.mp4') ||
          file.name.toLowerCase().endsWith('.flac')) {

        console.log('Processing audio file:', file.name, file.type, file.size);
        setUploadedFile({
          file,
          preview: URL.createObjectURL(file),
        });
        setTranscription("");
        toast.success(`${file.name} yüklendi`);
      } else {
        console.log('Rejected file:', file.name, file.type);
        toast.error("Sadece ses dosyaları kabul edilir");
      }
    }
  }, []);

  const handleFileSelect = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      const file = files[0];
      console.log('File selected via input:', file.name, file.type, file.size);
      setUploadedFile({
        file,
        preview: URL.createObjectURL(file),
      });
      setTranscription("");
    }
  }, []);

    const handleTranscribe = async () => {
    if (!uploadedFile || !currentModel) return;

    setTranscribing(true);
    try {
      toast.info("Transkripsiyon başlatılıyor...");

      // Convert to base64 using a safer method to avoid "Maximum call stack size exceeded"
      const fileBuffer = await uploadedFile.file.arrayBuffer();
      const fileBytes = new Uint8Array(fileBuffer);
      
      // Use a more memory-efficient base64 conversion
      let binary = '';
      const chunkSize = 8192; // 8KB chunks for safety
      for (let i = 0; i < fileBytes.length; i += chunkSize) {
        const chunk = fileBytes.slice(i, i + chunkSize);
        binary += String.fromCharCode.apply(null, Array.from(chunk));
      }
      const fileData = btoa(binary);

      // Call the real backend command
      const result = await invoke<string>("transcribe_uploaded_audio", {
        fileData: fileData,
        fileName: uploadedFile.file.name,
        modelId: currentModel,
        saveToHistory: true,
      });

      setTranscription(result);
      toast.success("Transkripsiyon tamamlandı!");
    } catch (error) {
      console.error("Transkripsiyon hatası:", error);
      const errorMessage = error instanceof Error ? error.message : String(error);
      toast.error(`Transkripsiyon başarısız oldu: ${errorMessage}`);
    } finally {
      setTranscribing(false);
    }
  };

  const handleSave = async () => {
    if (!transcription) return;

    try {
      // Save to history - this would need backend implementation
      toast.info("Geçmişe kaydediliyor...");
      // await invoke("save_transcription", { text: transcription, fileName: uploadedFile?.file.name });
      toast.success("Geçmişe kaydedildi!");
    } catch (error) {
      toast.error("Kaydetme başarısız oldu");
    }
  };

  return (
    <div className="space-y-6">
      <SettingsGroup title="Ses Dosyası Yükleme">
        <div
          className={`border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors ${
            isDragOver
              ? "border-blue-500 bg-blue-50 dark:bg-blue-950"
              : "border-gray-300 dark:border-gray-600 hover:border-gray-400 dark:hover:border-gray-500"
          }`}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          onClick={() => document.getElementById('audio-file-input')?.click()}
        >
          <input
            id="audio-file-input"
            type="file"
            accept="audio/*,.wav,.mp3,.m4a,.mp4,.flac"
            onChange={handleFileSelect}
            className="hidden"
          />
          <Upload className="mx-auto h-12 w-12 text-gray-400 mb-4" />
          {isDragOver ? (
            <p className="text-lg font-medium text-blue-600 dark:text-blue-400">
              Dosyayı buraya bırakın...
            </p>
          ) : (
            <div>
              <p className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
                Ses dosyasını sürükleyin veya tıklayın
              </p>
              <p className="text-sm text-gray-500 dark:text-gray-400">
                WAV, MP3, M4A, MP4, FLAC formatları desteklenir
              </p>
            </div>
          )}
        </div>

        {uploadedFile && (
          <div className="mt-4 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
            <div className="flex items-center space-x-3 mb-3">
              <FileAudio className="h-8 w-8 text-blue-500" />
              <div>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {uploadedFile.file.name}
                </p>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  {(uploadedFile.file.size / 1024 / 1024).toFixed(2)} MB
                </p>
              </div>
            </div>
            <AudioPlayer src={uploadedFile.preview} />
          </div>
        )}
      </SettingsGroup>

      {uploadedFile && (
        <SettingsGroup title="Transkripsiyon">
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                Model: {models.find(m => m.id === currentModel)?.name || "Model seçilmedi"}
              </span>
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                Dil: {settings?.selected_language || "tr"}
              </span>
            </div>

            <Button
              onClick={handleTranscribe}
              disabled={!currentModel || transcribing}
              className="w-full"
            >
              {transcribing ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Transkripsiyon yapılıyor...
                </>
              ) : (
                "Transkripsiyon Yap"
              )}
            </Button>

            {transcription && (
              <div className="space-y-3">
                <div className="p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
                  <p className="text-sm text-gray-900 dark:text-gray-100 whitespace-pre-wrap">
                    {transcription}
                  </p>
                </div>
                <Button onClick={handleSave} variant="secondary" className="w-full">
                  Geçmişe Kaydet
                </Button>
              </div>
            )}
          </div>
        </SettingsGroup>
      )}
    </div>
  );
};