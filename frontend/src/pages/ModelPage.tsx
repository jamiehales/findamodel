import { useQuery } from '@tanstack/react-query'
import { useParams, useNavigate } from 'react-router-dom'
import { fetchModels } from '../lib/api'
import styles from './ModelPage.module.css'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function ModelPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()

  const decodedId = decodeURIComponent(id ?? '')

  const { data: model, isPending, isError } = useQuery({
    queryKey: ['models'],
    queryFn: fetchModels,
    select: models => models.find(m => m.id === decodedId) ?? null,
  })

  if (isPending) {
    return (
      <div className={styles.container}>
        <div className={styles.loadingState}>
          <div className={styles.spinner} />
        </div>
      </div>
    )
  }

  if (isError || model === null) {
    return (
      <div className={styles.container}>
        <div className={styles.errorState}>
          <p>Model not found.</p>
          <button className={styles.backButton} onClick={() => navigate('/')}>← Back</button>
        </div>
      </div>
    )
  }

  return (
    <div className={styles.container}>
      <button className={styles.backButton} onClick={() => navigate('/')}>
        ← Back
      </button>

      <div className={styles.content}>
        <div className={styles.header}>
          <span className={`${styles.typeBadge} ${styles[model.fileType]}`}>
            {model.fileType.toUpperCase()}
          </span>
          <h1 className={styles.title}>{model.name}</h1>
          <p className={styles.path}>{model.relativePath}</p>
          <p className={styles.meta}>{formatBytes(model.fileSize)}</p>
        </div>

        <div className={styles.viewerPlaceholder}>
          <span className={styles.viewerIcon}>⬡</span>
          <p>3D viewer coming soon</p>
        </div>

        <a
          className={styles.downloadButton}
          href={model.fileUrl}
          download={`${model.name}.${model.fileType}`}
        >
          Download .{model.fileType}
        </a>
      </div>
    </div>
  )
}

export default ModelPage
