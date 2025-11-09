import { useState, useEffect, useRef } from 'react'
import './App.css'

function App() {
  const [stations, setStations] = useState([])
  const [currentFreqIndex, setCurrentFreqIndex] = useState(0)
  const [playing, setPlaying] = useState(false)
  const [nowPlaying, setNowPlaying] = useState(null)
  const [volume, setVolume] = useState(0.7)
  const [tuning, setTuning] = useState(false)
  const [audioLevels, setAudioLevels] = useState(new Array(16).fill(0))
  const audioRef = useRef(null)
  const nowPlayingEventSourceRef = useRef(null)
  const stationsEventSourceRef = useRef(null)
  const audioContextRef = useRef(null)
  const analyserRef = useRef(null)
  const animationFrameRef = useRef(null)

  // Fetch stations initially and subscribe to live updates
  useEffect(() => {
    // Initial fetch
    fetch('/api/v1/stations')
      .then(res => res.json())
      .then(data => {
        if (data && data.length > 0) {
          setStations(data)
        }
      })
      .catch(err => console.error('Failed to fetch stations:', err))

    // Subscribe to station registry updates
    const es = new EventSource('/api/v1/events')
    es.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data)
        if (event.event === 'upsert' && event.assignment) {
          // Add or update station
          setStations(prev => {
            const existing = prev.findIndex(s => s.station_id === event.assignment.station_id)
            if (existing >= 0) {
              const updated = [...prev]
              updated[existing] = event.assignment
              return updated
            } else {
              return [...prev, event.assignment]
            }
          })
        } else if (event.event === 'delete' && event.assignment) {
          // Remove station
          setStations(prev => {
            const filtered = prev.filter(s => s.station_id !== event.assignment.station_id)
            // If currently playing station was removed, adjust index
            setCurrentFreqIndex(idx => {
              if (idx >= filtered.length && filtered.length > 0) {
                return filtered.length - 1
              }
              return idx
            })
            return filtered
          })
        }
      } catch (err) {
        console.error('Failed to parse station event:', err)
      }
    }
    es.onerror = () => {
      console.error('Station events SSE connection error')
    }
    stationsEventSourceRef.current = es

    return () => {
      es.close()
    }
  }, [])

  const currentStation = stations[currentFreqIndex]
  const currentStationId = currentStation?.station_id
  const currentStreamUrl = currentStation?.stream_url

  // Subscribe to now-playing updates
  useEffect(() => {
    if (!currentStation || !playing) {
      if (nowPlayingEventSourceRef.current) {
        nowPlayingEventSourceRef.current.close()
        nowPlayingEventSourceRef.current = null
      }
      setNowPlaying(null)
      return
    }

    const streamUrl = new URL(currentStation.stream_url)
    const nowUrl = `${streamUrl.protocol}//${streamUrl.host}/api/v1/now/events`

    const es = new EventSource(nowUrl)
    es.onmessage = (e) => {
      try {
        const data = JSON.parse(e.data)
        setNowPlaying(data)
      } catch (err) {
        console.error('Failed to parse now-playing:', err)
      }
    }
    es.onerror = () => {
      console.error('Now-playing SSE connection error')
    }
    nowPlayingEventSourceRef.current = es

    return () => {
      es.close()
    }
  }, [currentStationId, currentStreamUrl, playing])

  // Audio playback and analyzer setup
  useEffect(() => {
    if (!audioRef.current) return

    if (playing && currentStreamUrl) {
      // Setup audio context and analyzer for visualizer
      if (!audioContextRef.current) {
        const AudioContext = window.AudioContext || window.webkitAudioContext
        audioContextRef.current = new AudioContext()
        analyserRef.current = audioContextRef.current.createAnalyser()
        analyserRef.current.fftSize = 64
        analyserRef.current.smoothingTimeConstant = 0.7
        
        const source = audioContextRef.current.createMediaElementSource(audioRef.current)
        source.connect(analyserRef.current)
        analyserRef.current.connect(audioContextRef.current.destination)
      }

      audioRef.current.src = currentStreamUrl
      audioRef.current.crossOrigin = 'anonymous'
      audioRef.current.play().catch(err => {
        console.error('Playback failed:', err)
        setPlaying(false)
      })

      // Start visualizer animation
      const updateVisualizer = () => {
        if (!analyserRef.current || !playing) return
        
        const bufferLength = analyserRef.current.frequencyBinCount
        const dataArray = new Uint8Array(bufferLength)
        analyserRef.current.getByteFrequencyData(dataArray)
        
        // Take 16 samples from lower 75% of frequency spectrum for better visualization
        const bars = 16
        const usableRange = Math.floor(bufferLength * 0.75)
        const levels = []
        for (let i = 0; i < bars; i++) {
          const index = Math.floor((i * usableRange) / bars)
          levels.push(dataArray[index] / 255)
        }
        
        setAudioLevels(levels)
        animationFrameRef.current = requestAnimationFrame(updateVisualizer)
      }
      updateVisualizer()
    } else {
      audioRef.current.pause()
      audioRef.current.src = ''
      
      // Stop visualizer animation
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current)
        animationFrameRef.current = null
      }
      setAudioLevels(new Array(16).fill(0))
    }

    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current)
      }
    }
  }, [playing, currentStreamUrl])

  // Handle volume changes separately (without reloading stream)
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = volume
    }
  }, [volume])

  const handleTuneUp = () => {
    if (stations.length === 0) return
    setTuning(true)
    setCurrentFreqIndex((prev) => (prev + 1) % stations.length)
    setTimeout(() => setTuning(false), 200)
  }

  const handleTuneDown = () => {
    if (stations.length === 0) return
    setTuning(true)
    setCurrentFreqIndex((prev) => (prev - 1 + stations.length) % stations.length)
    setTimeout(() => setTuning(false), 200)
  }

  const togglePlay = () => {
    if (stations.length === 0) return
    setPlaying(!playing)
  }

  const formatFrequency = (freq) => {
    if (!freq) return '---.-'
    const num = typeof freq === 'string' ? parseFloat(freq) : freq
    return num.toFixed(1)
  }

  const handleStationClick = (index) => {
    setCurrentFreqIndex(index)
  }

  return (
    <div className="radio">
      <audio ref={audioRef} />
      
      <div className="display-panel">
        {/* Album Art - left panel (only show if artwork available) */}
        {playing && nowPlaying?.cover_url && (
          <div className="album-art-container">
            <div className="scanlines" />
            <img 
              src={nowPlaying.cover_url} 
              alt="Album artwork"
              className="album-art"
            />
          </div>
        )}

        <div className="display">
          <div className="scanlines" />
          <div className="vfd-content">
            <div className="frequency-display">
              <span className="freq-label">FREQ</span>
              <span className={`freq-value ${tuning ? 'tuning' : ''}`}>
                {currentStation ? formatFrequency(currentStation.frequency) : '---.-'}
              </span>
              <span className="freq-unit">MHz</span>
            </div>
            
            <div className="station-info">
              <div className="station-name">
                {currentStation ? currentStation.name : 'NO SIGNAL'}
              </div>
              
              {playing && nowPlaying && (
                <div className="now-playing">
                  {nowPlaying.artist && (
                    <div className="np-artist">{nowPlaying.artist}</div>
                  )}
                  {nowPlaying.title && (
                    <div className="np-title">{nowPlaying.title}</div>
                  )}
                  {nowPlaying.album && (
                    <div className="np-album">{nowPlaying.album}</div>
                  )}
                </div>
              )}
              
              {!playing && currentStation && (
                <div className="status-indicator">STANDBY</div>
              )}
            </div>
          </div>
        </div>

        {/* Audio Visualizer - right panel */}
        <div className="visualizer-container">
          <div className="scanlines" />
          <div className="visualizer">
            {audioLevels.map((level, i) => {
              const segments = 32
              const activeSegments = Math.floor(level * segments)
              return (
                <div key={i} className="visualizer-bar">
                  {Array.from({ length: segments }).map((_, j) => (
                    <div
                      key={j}
                      className={`visualizer-segment ${j < activeSegments ? 'active' : ''}`}
                    />
                  ))}
                </div>
              )
            })}
          </div>
        </div>
      </div>

      <div className="controls">
        <button 
          className="tune-btn"
          onClick={handleTuneDown}
          disabled={stations.length === 0}
        >
          ◄◄
        </button>
        
        <button 
          className={`power-btn ${playing ? 'active' : ''}`}
          onClick={togglePlay}
          disabled={stations.length === 0}
        >
          {playing ? '■' : '▶'}
        </button>
        
        <button 
          className="tune-btn"
          onClick={handleTuneUp}
          disabled={stations.length === 0}
        >
          ►►
        </button>

        <div className="volume-control">
          <span className="vol-label">VOL</span>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={volume}
            onChange={(e) => setVolume(parseFloat(e.target.value))}
            className="volume-slider"
          />
          <span className="vol-value">{Math.round(volume * 100)}</span>
        </div>
      </div>

      {/* Station List */}
      <div className="station-list-container">
        <div className="station-list-header">
          AVAILABLE STATIONS ({stations.length})
        </div>
        <div className="station-list">
          {stations.length === 0 ? (
            <div className="station-list-empty">NO STATIONS AVAILABLE</div>
          ) : (
            stations.map((station, idx) => (
              <div
                key={station.station_id}
                className={`station-item ${idx === currentFreqIndex ? 'current' : ''}`}
                onClick={() => handleStationClick(idx)}
              >
                <div className="station-item-left">
                  <div className="station-item-freq">
                    {formatFrequency(station.frequency)}
                  </div>
                  <div className="station-item-name">
                    {station.name}
                  </div>
                </div>
                {idx === currentFreqIndex && playing && (
                  <div className="station-item-status">PLAYING</div>
                )}
                {idx === currentFreqIndex && !playing && (
                  <div className="station-item-status">SELECTED</div>
                )}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}

export default App

