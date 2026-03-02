import { useState } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Typography from '@mui/material/Typography'
import { useNavigate } from 'react-router-dom'
import ModelGrid from '../components/ModelGrid'
import ModelFilters from '../components/ModelFilters'
import IndexerStatus from '../components/IndexerStatus'
import { useActivePrintingList, useFilterOptions } from '../lib/queries'
import { emptyFilter, type ModelFilter } from '../lib/api'
import styles from './WelcomePage.module.css'

function WelcomePage() {
  const navigate = useNavigate()
  const { data: activeList } = useActivePrintingList()
  const { data: filterOptions } = useFilterOptions()
  const totalCount = activeList?.items.reduce((a, i) => a + i.quantity, 0) ?? 0

  const [filter, setFilter] = useState<ModelFilter>(emptyFilter)

  return (
    <>
    <IndexerStatus />
    <Box className={styles.page}>
      <Box className={styles.header}>
        <Typography component="h1" className={styles.logoTitle}>
          find
          <Box component="span" className={styles.logoA}>
            a
          </Box>
          model
        </Typography>
        <Typography color="text.secondary" className={styles.subtitle}>
          Find your next mini
        </Typography>
        <Box className={styles.actions}>
          <Button
            onClick={() => navigate('/explore')}
            variant="outlined"
            className={styles.btnOutlinePrimary}
          >
            Explore
          </Button>
          {totalCount > 0 && (
            <Button
              onClick={() => navigate('/printing-list/active')}
              variant="outlined"
              className={styles.btnOutlinePrimary}
            >
              View printing list ({totalCount})
            </Button>
          )}
          <Button
            onClick={() => navigate('/printing-lists')}
            variant="outlined"
            className={styles.btnOutlineNeutral}
          >
            Manage printing lists
          </Button>
        </Box>
      </Box>

      {filterOptions && (
        <ModelFilters value={filter} onChange={setFilter} options={filterOptions} />
      )}

      <ModelGrid filter={filter} />
    </Box>
    </>
  )
}

export default WelcomePage
