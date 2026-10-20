import { Navigate } from 'react-router'

import { landingPath } from '../landing'

/* The app's entry: straight on to where the build opens (landing.ts). */
function Index() {
 return <Navigate to={landingPath} replace />
}

export default Index
