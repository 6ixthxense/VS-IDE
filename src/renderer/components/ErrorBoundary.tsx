import React from 'react'

interface Props {
  children: React.ReactNode
  fallbackMessage?: string
}

interface State {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('[ErrorBoundary]', error, info.componentStack)
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: null })
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          padding: '24px',
          color: '#ff6b6b',
          backgroundColor: '#1a1a1a',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: '16px',
          fontFamily: 'Inter, system-ui, sans-serif',
        }}>
          <div style={{ fontSize: '36px' }}>⚠️</div>
          <h3 style={{ color: '#fff', margin: 0 }}>
            {this.props.fallbackMessage || 'Something went wrong'}
          </h3>
          <pre style={{
            fontSize: '11px',
            whiteSpace: 'pre-wrap',
            fontFamily: 'monospace',
            maxHeight: '200px',
            overflow: 'auto',
            background: 'rgba(0,0,0,0.3)',
            padding: '12px',
            borderRadius: '8px',
            maxWidth: '600px',
            width: '100%',
          }}>
            {this.state.error?.message || 'Unknown error'}
          </pre>
          <button
            onClick={this.handleRetry}
            style={{
              background: '#007acc',
              color: '#fff',
              border: 'none',
              padding: '8px 20px',
              borderRadius: '6px',
              cursor: 'pointer',
              fontSize: '13px',
              fontWeight: 500,
            }}
          >
            🔄 Retry
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
