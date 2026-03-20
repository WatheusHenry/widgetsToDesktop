import { useEffect, useState } from "react";
import "./App.css";

import {
  Sun, CloudSun, Cloud, CloudFog, CloudDrizzle, CloudRain,
  CloudSnow, CloudLightning, Wind, Headphones, Users, Volume2, CircleDashed, UserRound,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ─── Weather ─────────────────────────────────────────────────────────────────
function parseWeatherCode(code: number) {
  if (code === 0) return { desc: "Limpo", Icon: Sun };
  if (code === 1 || code === 2) return { desc: "Parcialmente Nublado", Icon: CloudSun };
  if (code === 3) return { desc: "Nublado", Icon: Cloud };
  if (code >= 45 && code <= 48) return { desc: "Nevoeiro", Icon: CloudFog };
  if (code >= 51 && code <= 55) return { desc: "Chuvisco", Icon: CloudDrizzle };
  if (code >= 61 && code <= 65) return { desc: "Chuva", Icon: CloudRain };
  if (code >= 71 && code <= 77) return { desc: "Neve", Icon: CloudSnow };
  if (code >= 95) return { desc: "Tempestade", Icon: CloudLightning };
  return { desc: "Vento", Icon: Wind };
}

function WeatherWidget() {
  const [weather, setWeather] = useState<{ temp: number; desc: string; Icon: React.ElementType } | null>(null);

  useEffect(() => {
    async function fetchWeather(lat: number, lon: number) {
      try {
        const res = await fetch(`https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&current=temperature_2m,weather_code`);
        const data = await res.json();
        const w = parseWeatherCode(data.current.weather_code);
        setWeather({ temp: Math.round(data.current.temperature_2m), desc: w.desc, Icon: w.Icon });
      } catch (e) { console.error(e); }
    }
    if (navigator.geolocation) {
      navigator.geolocation.getCurrentPosition(
        (pos) => fetchWeather(pos.coords.latitude, pos.coords.longitude),
        () => fetchWeather(-23.5489, -46.6388),
      );
    } else {
      fetchWeather(-23.5489, -46.6388);
    }
  }, []);

  return (
    <div className="widget-container weather-widget" data-tauri-drag-region>
      {weather ? (
        <div className="weather-content" data-tauri-drag-region>
          <div className="weather-left" data-tauri-drag-region>
            <span className="city-name" data-tauri-drag-region>Marília - SP</span>
            <span className="heavy-temp" data-tauri-drag-region>{weather.temp}°</span>
          </div>
          <div className="weather-right" data-tauri-drag-region>
            <weather.Icon size={18} color="white" strokeWidth={2.5} style={{ opacity: 0.95 }} />
            <span className="weather-desc" data-tauri-drag-region>{weather.desc}</span>
          </div>
        </div>
      ) : (
        <span className="weather-loading" data-tauri-drag-region>Carregando...</span>
      )}
    </div>
  );
}

// ─── Discord ──────────────────────────────────────────────────────────────────
function getDefaultAvatar(userId: string): string {
  const index = Number(BigInt(userId) >> 22n) % 6;
  return `https://cdn.discordapp.com/embed/avatars/${index}.png`;
}

function DiscordWidget() {
  const [channelMembers, setChannelMembers] = useState<any[] | null>(null);
  const [rpcStatus, setRpcStatus] = useState<string>("Conectando...");

  useEffect(() => {
    const unlistenStatus = listen<string>("rpc-status", (e) => setRpcStatus(e.payload));
    const unlistenMembers = listen<any[]>("rpc-members", (e) => {
      setChannelMembers(e.payload);
      setRpcStatus("");
    });
    return () => {
      unlistenStatus.then(fn => fn());
      unlistenMembers.then(fn => fn());
    };
  }, []);

  const handleJoin = async () => {
    try { await invoke("join_discord"); }
    catch (e) { console.error(e); }
  };

  return (
    <div className="widget-container discord-widget" data-tauri-drag-region>
      <div className="discord-content" data-tauri-drag-region>

        <div className="discord-top" data-tauri-drag-region>
          <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
            <svg viewBox="0 0 127.14 96.36" fill="white" width="16" height="16" data-tauri-drag-region>
              <path d="M107.7,8.07A105.15,105.15,0,0,0,81.47,0a72.06,72.06,0,0,0-3.36,6.83A97.68,97.68,0,0,0,49,6.83,72.37,72.37,0,0,0,45.64,0,105.89,105.89,0,0,0,19.39,8.09C2.79,32.65-1.71,56.6.54,80.21h0A105.73,105.73,0,0,0,32.71,96.36,77.7,77.7,0,0,0,39.6,85.25a68.42,68.42,0,0,1-10.85-5.18c.91-.66,1.8-1.34,2.66-2a75.57,75.57,0,0,0,64.32,0c.87.71,1.76,1.39,2.66,2a68.68,68.68,0,0,1-10.87,5.19,77.7,77.7,0,0,0,6.89,11.1,105.25,105.25,0,0,0,32.19-16.14h0C127.86,52.43,121.36,29,107.7,8.07ZM42.45,65.69C36.18,65.69,31,60,31,53s5-12.74,11.43-12.74S54,46,53.89,53,48.84,65.69,42.45,65.69Zm42.24,0C78.41,65.69,73.31,60,73.31,53s5-12.74,11.43-12.74S96.2,46,96.12,53,91.08,65.69,84.69,65.69Z" data-tauri-drag-region />
            </svg>
            <span className="discord-title">Discord</span>
          </div>
        </div>
        <span className="discord-playing" data-tauri-drag-region>
          <div style={{ display: "flex", alignItems: "center" }}>
            <Volume2 size={16} style={{ marginRight: "6px", verticalAlign: "middle" }} />
            jogando
          </div>
          <span style={{ color: "rgba(5, 5, 5, 1)", backgroundColor: "#ffffff4f", padding: "2px 5px", borderRadius: "20px" }}>
            {channelMembers !== null && !rpcStatus ? channelMembers.length : "?"}
          </span>

        </span>
        <div style={{ display: "flex", alignItems: "center", gap: "4px", padding: "12px 12px" }}>
          <div className="discord-mid" data-tauri-drag-region>
            {rpcStatus ? (
              <span className="members-error" data-tauri-drag-region>{rpcStatus}</span>
            ) : channelMembers && channelMembers.length > 0 ? (
              <div className="avatars-container" data-tauri-drag-region>
                {channelMembers.length === 1 ? (
                  <div style={{ display: "flex", alignItems: "center", gap: "8px" }} data-tauri-drag-region>
                    <img
                      src={channelMembers[0].avatar
                        ? `https://cdn.discordapp.com/avatars/${channelMembers[0].id}/${channelMembers[0].avatar}.png`
                        : getDefaultAvatar(channelMembers[0].id)}
                      alt={channelMembers[0].username}
                      className="member-avatar"
                      data-tauri-drag-region
                    />
                    <span style={{ fontSize: "11px", fontWeight: "600", color: "white", maxWidth: "70px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} data-tauri-drag-region>
                      {channelMembers[0].username}
                    </span>
                  </div>
                ) : (
                  <>
                    {channelMembers.slice(0, 3).map((user: any, i: number) => {
                      const avatarUrl = user.avatar
                        ? `https://cdn.discordapp.com/avatars/${user.id}/${user.avatar}.png`
                        : getDefaultAvatar(user.id);
                      return (
                        <img key={user.id} src={avatarUrl} alt={user.username} title={user.username}
                          className="member-avatar" style={{ zIndex: 10 - i }} data-tauri-drag-region />
                      );
                    })}
                    {channelMembers.length > 3 && (
                      <div className="member-avatar more-avatar" data-tauri-drag-region>
                        +{channelMembers.length - 3}
                      </div>
                    )}
                  </>
                )}
              </div>
            ) : channelMembers && channelMembers.length === 0 ? (
              <span className="empty-call" data-tauri-drag-region>
                <div style={{ position: "relative", width: "24px", height: "24px", display: "flex", alignItems: "center", justifyContent: "center", opacity: 0.5 }}>
                  <CircleDashed size={24} style={{ position: "absolute" }} />
                  <UserRound size={14} />
                </div>
                vazio
              </span>
            ) : (
              <span className="empty-call" data-tauri-drag-region>...</span>
            )}
          </div>

          <button className="join-btn" onClick={handleJoin}>
            <Headphones size={12} /> Entrar
          </button>

        </div>

      </div>
    </div >
  );
}

// ─── App ──────────────────────────────────────────────────────────────────────
function App() {
  const [time, setTime] = useState(new Date());

  useEffect(() => {
    const id = setInterval(() => setTime(new Date()), 1000);
    return () => clearInterval(id);
  }, []);

  const hours = time.getHours().toString().padStart(2, "0");
  const minutes = time.getMinutes().toString().padStart(2, "0");

  return (
    <div className="layout" data-tauri-drag-region>

      {/* Linha de cima: discord (esquerda) + relógio (direita) */}
      <div className="top-row" data-tauri-drag-region>
        <DiscordWidget />
        <div className="widget-container" data-tauri-drag-region>
          <div className="digital-clock" data-tauri-drag-region>
            <span className="time" data-tauri-drag-region>{hours}:{minutes}</span>
          </div>
        </div>
      </div>

      {/* Linha de baixo: clima quadrado */}
      <WeatherWidget />

    </div>
  );
}

export default App;