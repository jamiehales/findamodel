import ModelGrid from '../components/ModelGrid'
import styles from './WelcomePage.module.css'

function WelcomePage() {
  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <h1 className={styles.title}>find<span className={styles.logoA}>a</span>model</h1>
        <p className={styles.subtitle}>Find your next mini</p>
      </div>
      <ModelGrid />
    </div>
  )
}

export default WelcomePage
