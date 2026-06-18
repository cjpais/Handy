import os
import json

def main():
    transcripts_map = {}
    
    clean_folders = [
        ("single_speaker/clean/malayalam_only", False, False),
        ("single_speaker/clean/code_switched", False, True),
        ("multi_speaker/clean/malayalam_only", True, False),
        ("multi_speaker/clean/code_switched", True, True)
    ]
    
    # Text transcripts database to reconstruct original text if needed
    transcripts_db = {
        # Single Malayalam
        "mal_single_01": "കേരളത്തിലെ കാലാവസ്ഥ വളരെ മനോഹരമാണ്.",
        "mal_single_02": "മലയാള ഭാഷ എനിക്ക് വളരെ ഇഷ്ടമാണ്.",
        "mal_single_03": "നാളെ രാവിലെ നമുക്ക് തിരുവനന്തപുരത്തേക്ക് പോകാം.",
        "mal_single_04": "ഈ ആപ്ലിക്കേഷൻ എങ്ങനെയാണ് ഉപയോഗിക്കേണ്ടത് എന്ന് പറയാമോ?",
        "mal_single_05": "ഇന്നത്തെ വാർത്തകളിൽ പ്രധാനപ്പെട്ടത് എന്തൊക്കെയാണ്?",
        "mal_single_06": "കേരളത്തിൽ കാണേണ്ട പ്രധാന സ്ഥലങ്ങൾ ഏതൊക്കെയാണ്?",
        "mal_single_07": "എനിക്ക് ഒരു ഗ്ലാസ്സ് വെള്ളം തരുമോ?",
        "mal_single_08": "അടുത്ത ബസ്സ് എപ്പോഴാണ് വരുന്നത് എന്ന് അറിയാമോ?",
        "mal_single_09": "ഈ വർഷത്തെ മഴ വളരെ കൂടുതലായിരുന്നു.",
        "mal_single_10": "എല്ലാവർക്കും എന്റെ ഹൃദയം നിറഞ്ഞ ഓണാശംസകൾ!",
        # Single CS
        "cs_single_01": "എന്റെ phone-ന്റെ battery തീരാറായി.",
        "cs_single_02": "ആ project-ന്റെ deadline നാളെയാണെന്ന് ഓർമ്മയുണ്ടോ?",
        "cs_single_03": "നാളെ നമുക്ക് ഒരു discussion ഉണ്ട്.",
        "cs_single_04": "ഈ laptop വളരെ slow ആണ്, എനിക്ക് പുതിയത് വാങ്ങണം.",
        "cs_single_05": "അവൻ നല്ലൊരു software engineer ആണ്.",
        # Multi Malayalam
        "mal_multi_01": "Speaker 1: ഹലോ, സുഖമാണോ? കുറെ നാളായല്ലോ കണ്ടിട്ട്.\nSpeaker 2: അതെ സുഖമാണ്! നീ എവിടെയായിരുന്നു ഇത്രയും നാൾ?",
        "mal_multi_02": "Speaker 1: ഇന്നത്തെ കാലാവസ്ഥ എങ്ങനെയുണ്ട്? മഴ പെയ്യാൻ സാധ്യതയുണ്ടോ?\nSpeaker 2: ആകാശം കറുത്തിരുണ്ടിരിക്കുകയാണ്, എപ്പോൾ വേണമെങ്കിലും മഴ പെയ്യാം.",
        "mal_multi_03": "Speaker 1: നാളത്തെ യാത്രയ്ക്കുള്ള ഒരുക്കങ്ങൾ ഒക്കെ കഴിഞ്ഞോ?\nSpeaker 2: അതെ, ബാഗ് എല്ലാം പാക്ക് ചെയ്തു. ടിക്കറ്റും റെഡിയാണ്."
    }
    
    for folder, is_multi, is_cs in clean_folders:
        full_path = os.path.join("benchmark", folder)
        if not os.path.exists(full_path):
            continue
            
        files = [f for f in os.listdir(full_path) if f.endswith(".wav")]
        for file in files:
            file_id = file.replace(".wav", "")
            txt_path = os.path.join(full_path, file_id + ".txt")
            
            clean_text = ""
            if os.path.exists(txt_path):
                with open(txt_path, "r", encoding="utf-8") as f:
                    clean_text = f.read().strip()
                    
            original_text = transcripts_db.get(file_id, clean_text)
            
            transcripts_map[file_id] = {
                "original_text": original_text,
                "clean_text": clean_text,
                "is_multi_speaker": is_multi,
                "is_code_switched": is_cs
            }
            
    # Write to transcripts.json
    out_path = "benchmark/transcripts.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(transcripts_map, f, ensure_ascii=False, indent=2)
        
    print(f"Successfully generated transcripts mapping for {len(transcripts_map)} files at {out_path}", flush=True)

if __name__ == "__main__":
    main()
