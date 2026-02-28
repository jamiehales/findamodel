import { useQuery } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { fetchModels } from '../lib/api'
import styles from './ModelGrid.module.css'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function ModelGrid() {
  const navigate = useNavigate()
  const { data: models, isPending, isError } = useQuery({
    queryKey: ['models'],
    queryFn: fetchModels,
  })

  if (isPending) {
    return (
      <div className={styles.section}>
        <h2 className={styles.sectionTitle}>Models</h2>
        <div className={styles.grid}>
          {[1, 2, 3, 4, 5, 6].map(i => <div key={i} className={styles.skeleton} />)}
        </div>
      </div>
    )
  }

  if (isError || !models || models.length === 0) return null

  return (
    <div className={styles.section}>
      <h2 className={styles.sectionTitle}>Models</h2>
      <div className={styles.grid}>
        {models.map(model => (
          <button
            key={model.id}
            className={styles.card}
            onClick={() => navigate(`/model/${encodeURIComponent(model.id)}`)}
          >
            {model.previewUrl && (
              <img src={model.previewUrl} className={styles.preview} alt="" />
            )}
            <div className={styles.cardBody}>
              <span className={`${styles.typeBadge} ${styles[model.fileType]}`}>
                {model.fileType.toUpperCase()}
              </span>
              <p className={styles.cardTitle}>{model.name}</p>
              <p className={styles.cardMeta}>{formatBytes(model.fileSize)}</p>
            </div>
          </button>
        ))}
      </div>
    </div>
  )
}

export default ModelGrid
