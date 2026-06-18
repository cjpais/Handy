import os
import struct
import random
import shutil

def add_noise_to_wav(in_wav_path, out_wav_path, noise_level=0.03):
    # Reads a WAV file, adds random noise, and writes it to the output path
    with open(in_wav_path, 'rb') as f:
        wav_data = f.read()
    
    # Standard WAV header is 44 bytes
    header = wav_data[:44]
    pcm_bytes = wav_data[44:]
    
    num_samples = len(pcm_bytes) // 2
    if num_samples == 0:
        # If file is empty or corrupted
        shutil.copyfile(in_wav_path, out_wav_path)
        return
        
    samples = list(struct.unpack(f'<{num_samples}h', pcm_bytes))
    
    max_val = 32767
    min_val = -32768
    
    noisy_samples = []
    # Using random.gauss (Gaussian distribution) for natural white noise profile
    for s in samples:
        noise = random.gauss(0, noise_level * max_val)
        ns = int(s + noise)
        
        # Clip values to fit in signed 16-bit short
        if ns > max_val:
            ns = max_val
        elif ns < min_val:
            ns = min_val
        noisy_samples.append(ns)
        
    noisy_pcm_bytes = struct.pack(f'<{num_samples}h', *noisy_samples)
    
    with open(out_wav_path, 'wb') as f:
        f.write(header + noisy_pcm_bytes)

def main():
    clean_folders = [
        "benchmark/single_speaker/clean/malayalam_only",
        "benchmark/single_speaker/clean/code_switched",
        "benchmark/multi_speaker/clean/malayalam_only",
        "benchmark/multi_speaker/clean/code_switched"
    ]
    
    print("Starting noise addition...", flush=True)
    
    total_files = 0
    for clean_dir in clean_folders:
        noisy_dir = clean_dir.replace("/clean/", "/noisy/")
        os.makedirs(noisy_dir, exist_ok=True)
        
        # Find all WAV files in clean folder
        files = [f for f in os.listdir(clean_dir) if f.endswith(".wav")]
        for file in files:
            clean_wav = os.path.join(clean_dir, file)
            noisy_wav = os.path.join(noisy_dir, file)
            
            # Add noise
            print(f"Adding noise: {clean_wav} -> {noisy_wav}...", end="", flush=True)
            add_noise_to_wav(clean_wav, noisy_wav, noise_level=0.03)
            print(" [DONE]", flush=True)
            
            # Also copy the txt transcript next to the noisy audio file
            txt_file = file.replace(".wav", ".txt")
            clean_txt = os.path.join(clean_dir, txt_file)
            noisy_txt = os.path.join(noisy_dir, txt_file)
            
            if os.path.exists(clean_txt):
                shutil.copyfile(clean_txt, noisy_txt)
                
            total_files += 1
            
    print(f"\nNoise addition completed successfully. Processed {total_files} files.", flush=True)

if __name__ == "__main__":
    main()
