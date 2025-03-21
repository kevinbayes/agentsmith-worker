import './App.css'
import PrivateRoute from "./pages/index.jsx";
import {Box} from "@mui/material";
import {FullPageContainer} from "./components/Page/index.jsx";
import {useNavigate} from "react-router-dom";

function App() {
  return (
      <FullPageContainer>
        <PrivateRoute />
      </FullPageContainer>
  )
}

export default App
